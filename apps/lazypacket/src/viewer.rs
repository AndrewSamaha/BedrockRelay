mod packet_logger;
mod protocol;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use packet_logger::{PacketDirection, PacketEntry};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use serde_json;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use uuid::Uuid;

struct SessionLog {
    path: PathBuf,
    session_id: Uuid,
    packets: Vec<PacketEntry>,
    start_time: i64,
    protocol_version: Option<String>,
}

impl SessionLog {
    fn load(path: PathBuf) -> Result<Self> {
        // Try to parse session ID from filename
        let filename = path
            .file_stem()
            .and_then(|s| s.to_str())
            .context("Invalid log filename")?;
        
        // Filename should be the session ID
        let session_id = Uuid::parse_str(filename)
            .context("Failed to parse session ID from filename")?;

        // Read log file (uncompressed)
        let data = fs::read(&path)
            .context("Failed to read log file")?;

        // Deserialize all packets
        // First, try new format: [u32 length][bincode serialized PacketEntry]
        // If that fails, try old format: just bincode serialized PacketEntry entries back-to-back
        use std::io::{Cursor, Read};
        let mut cursor = Cursor::new(&data);
        let mut packets = Vec::new();
        let mut start_time = None;
        let mut protocol_version = None;
        
        // Try new format first (with length prefix)
        loop {
            let position = cursor.position() as usize;
            
            // Check if we're at the end (need at least 4 bytes for length prefix)
            if data.len().saturating_sub(position) < 4 {
                break;
            }
            
            // Read the length prefix (4 bytes, little-endian u32)
            let mut len_bytes = [0u8; 4];
            if cursor.read_exact(&mut len_bytes).is_err() {
                break; // End of file
            }
            
            let entry_len = u32::from_le_bytes(len_bytes) as usize;
            
            // Sanity check: entry_len should be reasonable
            // - Must be > 0
            // - Must be <= 10MB (reasonable max packet size)
            // - Must fit in remaining data
            let current_position = cursor.position() as usize;
            let remaining = data.len().saturating_sub(current_position);
            
            if entry_len == 0 || entry_len > 10_000_000 || entry_len > remaining {
                // This doesn't look like a valid length prefix - might be old format
                cursor.set_position(position as u64);
                break;
            }
            
            // Read the serialized entry
            let mut entry_data = vec![0u8; entry_len];
            if cursor.read_exact(&mut entry_data).is_err() {
                cursor.set_position(position as u64);
                break;
            }
            
            // Deserialize the entry
            match bincode::deserialize::<PacketEntry>(&entry_data) {
                Ok(entry) => {
                    if start_time.is_none() {
                        start_time = Some(entry.timestamp);
                    }
                    // Extract protocol version from first packet that has it
                    if protocol_version.is_none() && entry.protocol_version.is_some() {
                        protocol_version = entry.protocol_version.clone();
                    }
                    packets.push(entry);
                }
                Err(_) => {
                    // Deserialization failed - this might be old format
                    if !packets.is_empty() {
                        // We've read some packets successfully, stop here
                        break;
                    }
                    // Reset and try old format
                    cursor.set_position(position as u64);
                    break;
                }
            }
        }
        
        // If we didn't read any packets with the new format, try old format
        // Old format: entries are written sequentially with bincode::serialize()
        // Bincode writes entries back-to-back, so we need to read them one at a time
        // The tricky part is that bincode doesn't know where entries end, so we use
        // deserialize_from which reads exactly one entry
        if packets.is_empty() && data.len() > 0 {
            cursor.set_position(0);
            
            // Try reading entries one at a time
            // Bincode's deserialize_from will read exactly one entry and stop
            let mut last_success_pos = 0;
            
            while (cursor.position() as usize) < data.len() {
                let pos_before = cursor.position() as usize;
                
                // Try to deserialize one entry from current position
                match bincode::deserialize_from::<_, PacketEntry>(&mut cursor) {
                    Ok(entry) => {
                        let pos_after = cursor.position() as usize;
                        
                        // Check if we actually advanced (read some data)
                        if pos_after > pos_before {
                            if start_time.is_none() {
                                start_time = Some(entry.timestamp);
                            }
                            packets.push(entry);
                            last_success_pos = pos_after;
                            
                            // If we've consumed all data, we're done
                            if pos_after >= data.len() {
                                break;
                            }
                        } else {
                            // Didn't advance - something wrong, stop
                            break;
                        }
                    }
                    Err(e) => {
                        // Deserialization failed
                        // If we've read some packets, we're probably done
                        if !packets.is_empty() {
                            // We successfully read some packets, stop here
                            break;
                        }
                        
                        // If we haven't read anything yet, check if this looks like new format
                        // by checking if first 4 bytes could be a valid length prefix
                        cursor.set_position(0);
                        let first_four = if data.len() >= 4 {
                            u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize
                        } else {
                            0
                        };
                        
                        // If first 4 bytes look like a reasonable length (and we have that much data),
                        // this is probably a new format file that failed validation
                        if first_four > 0 && first_four <= data.len() && first_four <= 10_000_000 {
                            return Err(anyhow::anyhow!(
                                "File appears to be in new format (with length prefixes) but failed to read. First length value: {} bytes, file size: {} bytes. Error: {}",
                                first_four,
                                data.len(),
                                e
                            ));
                        }
                        
                        // Otherwise, it's probably corrupted or wrong format
                        return Err(anyhow::anyhow!(
                            "Failed to read any packets. File may be corrupted, empty, or in an unsupported format. File size: {} bytes. Error: {}",
                            data.len(),
                            e
                        ));
                    }
                }
            }
        }
        
        // If we still have no packets, provide a detailed error
        if packets.is_empty() {
            // Check if file might be empty or very small
            if data.len() < 4 {
                return Err(anyhow::anyhow!(
                    "Log file is too small ({} bytes) to contain any packets",
                    data.len()
                ));
            }
            
            // Show first few bytes for debugging
            let preview = data.iter().take(16).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");
            return Err(anyhow::anyhow!(
                "No packets found in log file. File size: {} bytes. First 16 bytes (hex): {}",
                data.len(),
                preview
            ));
        }

        Ok(Self {
            path,
            session_id,
            packets,
            start_time: start_time.unwrap_or(0),
            protocol_version,
        })
    }

    fn relative_time(&self, timestamp: i64) -> i64 {
        timestamp - self.start_time
    }
}

struct ViewerApp {
    logs_dir: PathBuf,
    sessions: Vec<(PathBuf, Uuid, usize)>, // path, session_id, packet_count
    selected_session: usize,
    current_log: Option<SessionLog>,
    packet_index: usize,
    mode: ViewerMode,
    error_message: Option<String>,
    show_hex: bool, // Toggle between JSON (default) and hex view
    packet_details_scroll: u16, // Scroll offset for packet details panel
    protocol_parser: Option<protocol::ProtocolParser>, // Loaded protocol parser
}

enum ViewerMode {
    SessionList,
    PacketView,
}

impl ViewerApp {
    fn new() -> Result<Self> {
        let logs_dir = PathBuf::from("logs");
        
        // Scan for log files
        let mut sessions = Vec::new();
        if logs_dir.exists() {
            for entry in fs::read_dir(&logs_dir)? {
                let entry = entry?;
                let path = entry.path();
                
                // Check if it's a log file (.bin)
                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    if ext == "bin" {
                        // Try to parse session ID from filename
                        if let Some(filename) = path.file_stem().and_then(|s| s.to_str()) {
                            if let Ok(session_id) = Uuid::parse_str(filename) {
                                // Quick count of packets (approximate)
                                let packet_count = Self::estimate_packet_count(&path).unwrap_or(0);
                                sessions.push((path, session_id, packet_count));
                            }
                        }
                    }
                }
            }
        }
        
        // Sort by modification time (newest first)
        sessions.sort_by(|a, b| {
            let time_a = fs::metadata(&a.0).and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            let time_b = fs::metadata(&b.0).and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            time_b.cmp(&time_a)
        });

        // Try to load protocol parser for default version
        let protocol_parser = protocol::ProtocolParser::new("1.21.111").ok();
        
        Ok(Self {
            logs_dir,
            sessions,
            selected_session: 0,
            current_log: None,
            packet_index: 0,
            mode: ViewerMode::SessionList,
            error_message: None,
            show_hex: false, // JSON by default
            packet_details_scroll: 0,
            protocol_parser,
        })
    }

    fn estimate_packet_count(path: &Path) -> Result<usize> {
        // Quick estimate - just check file size
        let metadata = fs::metadata(path)?;
        // Rough estimate: average packet entry might be ~100-200 bytes
        Ok((metadata.len() / 150) as usize)
    }

    fn load_session(&mut self) -> Result<()> {
        if let Some((path, _, _)) = self.sessions.get(self.selected_session) {
            let log = SessionLog::load(path.clone())?;
            self.current_log = Some(log);
            self.packet_index = 0;
            self.mode = ViewerMode::PacketView;
            Ok(())
        } else {
            Err(anyhow::anyhow!("No session selected"))
        }
    }

    fn current_packet(&self) -> Option<&PacketEntry> {
        self.current_log.as_ref()?.packets.get(self.packet_index)
    }

    fn prev_packet(&mut self) {
        if let Some(log) = &self.current_log {
            if self.packet_index > 0 {
                self.packet_index -= 1;
                // Reset scroll when packet changes
                self.packet_details_scroll = 0;
            }
        }
    }

    fn next_packet(&mut self) {
        if let Some(log) = &self.current_log {
            if self.packet_index < log.packets.len().saturating_sub(1) {
                self.packet_index += 1;
                // Reset scroll when packet changes
                self.packet_details_scroll = 0;
            }
        }
    }
}

fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = ViewerApp::new()?;
    let mut should_quit = false;

    while !should_quit {
        terminal.draw(|f| ui(f, &mut app))?;

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match app.mode {
                        ViewerMode::SessionList => {
                            match key.code {
                                KeyCode::Char('q') | KeyCode::Esc => should_quit = true,
                                KeyCode::Up => {
                                    if app.selected_session > 0 {
                                        app.selected_session -= 1;
                                    }
                                }
                                KeyCode::Down => {
                                    if app.selected_session < app.sessions.len().saturating_sub(1) {
                                        app.selected_session += 1;
                                    }
                                }
                                KeyCode::Enter => {
                                    app.error_message = None;
                                    if let Err(e) = app.load_session() {
                                        app.error_message = Some(format!("Failed to load session: {}", e));
                                    }
                                }
                                _ => {}
                            }
                        }
                        ViewerMode::PacketView => {
                            match key.code {
                                KeyCode::Char('q') | KeyCode::Esc => {
                                    app.mode = ViewerMode::SessionList;
                                    app.current_log = None;
                                }
                                KeyCode::Left | KeyCode::Char('h') => {
                                    app.prev_packet();
                                }
                                KeyCode::Right | KeyCode::Char('l') => {
                                    app.next_packet();
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    // Scroll up in packet details
                                    // Always allow decrementing - it will be clamped during rendering if needed
                                    if app.packet_details_scroll > 0 {
                                        app.packet_details_scroll -= 1;
                                    }
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    // Scroll down in packet details
                                    // We'll clamp this during rendering based on actual content
                                    app.packet_details_scroll += 1;
                                }
                                KeyCode::PageUp => {
                                    // Jump back 10 packets
                                    let old_index = app.packet_index;
                                    for _ in 0..10 {
                                        app.prev_packet();
                                    }
                                    // Reset scroll if packet actually changed
                                    if app.packet_index != old_index {
                                        app.packet_details_scroll = 0;
                                    }
                                }
                                KeyCode::PageDown => {
                                    // Jump forward 10 packets
                                    let old_index = app.packet_index;
                                    for _ in 0..10 {
                                        app.next_packet();
                                    }
                                    // Reset scroll if packet actually changed
                                    if app.packet_index != old_index {
                                        app.packet_details_scroll = 0;
                                    }
                                }
                                KeyCode::Home => {
                                    app.packet_index = 0;
                                    app.packet_details_scroll = 0;
                                }
                                KeyCode::End => {
                                    if let Some(log) = &app.current_log {
                                        app.packet_index = log.packets.len().saturating_sub(1);
                                        app.packet_details_scroll = 0;
                                    }
                                }
                                KeyCode::Char('x') | KeyCode::Char('X') => {
                                    // Toggle between JSON and hex view
                                    app.show_hex = !app.show_hex;
                                    // Reset scroll when toggling view
                                    app.packet_details_scroll = 0;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn ui(f: &mut Frame, app: &mut ViewerApp) {
    match app.mode {
        ViewerMode::SessionList => render_session_list(f, app),
        ViewerMode::PacketView => render_packet_view(f, app), // app is already &mut ViewerApp here
    }
}

fn render_session_list(f: &mut Frame, app: &ViewerApp) {
    let chunks = if app.error_message.is_some() {
        Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(f.size())
    } else {
        Layout::default()
            .constraints([Constraint::Min(0)])
            .split(f.size())
    };
    
    let main_area = if app.error_message.is_some() {
        chunks[1]
    } else {
        chunks[0]
    };
    
    // Show error message if present
    if let Some(ref error) = app.error_message {
        let error_paragraph = Paragraph::new(error.as_str())
            .block(Block::default().borders(Borders::ALL).title("Error").style(Style::default().fg(Color::Red)))
            .wrap(Wrap { trim: false });
        f.render_widget(error_paragraph, chunks[0]);
    }

    let items: Vec<ListItem> = app
        .sessions
        .iter()
        .map(|(_path, session_id, packet_count)| {
            let text = format!("{} ({} packets)", session_id, packet_count);
            ListItem::new(text)
        })
        .collect();

    use ratatui::widgets::ListState;
    let mut list_state = ListState::default();
    list_state.select(Some(app.selected_session));

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Session Logs (?? to navigate, Enter to select, q to quit)"))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    
    f.render_stateful_widget(list, main_area, &mut list_state);
}

fn render_packet_view(f: &mut Frame, app: &mut ViewerApp) {
    let log = match &app.current_log {
        Some(log) => log,
        None => return,
    };

    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(3), // Timeline
            Constraint::Min(0),    // Packet details
        ])
        .split(f.size());

    // Header
    let packet = app.current_packet();
    let packet_num = app.packet_index + 1;
    let total_packets = log.packets.len();
    
    let session_time = if let Some(p) = packet {
        let relative = log.relative_time(p.timestamp);
        format!("{:.3}s", relative as f64 / 1000.0)
    } else {
        "0.000s".to_string()
    };

    let view_mode = if app.show_hex { "HEX" } else { "JSON" };
    let version_str = log.protocol_version.as_ref()
        .map(|v| format!("Protocol: {}", v))
        .unwrap_or_else(|| "Protocol: Unknown".to_string());
    let header_text = format!(
        "Session: {} | {} | Packet: {}/{} | Time: {} | View: {} | [?/?/h/l: navigate, ?/?/k/j: scroll details, PgUp/PgDn: jump 10, Home/End: first/last, x: view, q: back]",
        log.session_id,
        version_str,
        packet_num,
        total_packets,
        session_time,
        view_mode
    );

    let header = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::ALL).title("Packet Viewer"));
    f.render_widget(header, chunks[0]);

    // Timeline visualization
    render_timeline(f, chunks[1], app);

    // Packet details
    if let Some(packet) = packet {
        let direction_str = match packet.direction {
            PacketDirection::Clientbound => "? Clientbound",
            PacketDirection::Serverbound => "? Serverbound",
        };
        
        let direction_color = match packet.direction {
            PacketDirection::Clientbound => Color::Green,
            PacketDirection::Serverbound => Color::Blue,
        };

        let timestamp_dt = DateTime::<Utc>::from_timestamp_millis(packet.timestamp)
            .unwrap_or_default();
        let time_str = timestamp_dt.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string();

        let details = if app.show_hex {
            // Hex view
            format!(
                "Direction: {}\nTimestamp: {}\nSize: {} bytes\n\nHex Dump:\n{}",
                direction_str,
                time_str,
                packet.data.len(),
                hex_dump(&packet.data, 16)
            )
        } else {
            // JSON view (default) - try to decode packet if parser is available
            let mut json_value = serde_json::json!({
                "direction": direction_str,
                "timestamp": packet.timestamp,
                "timestamp_formatted": time_str,
                "relative_time_ms": log.relative_time(packet.timestamp),
                "size_bytes": packet.data.len(),
            });
            
            // Try to decode packet using protocol parser
            if let Some(ref parser) = app.protocol_parser {
                let decoded = parser.decode_packet(&packet.data, packet.direction);
                
                if let Some(packet_name) = decoded.packet_name {
                    json_value["packet_name"] = serde_json::json!(packet_name);
                }
                if let Some(packet_id) = decoded.packet_id {
                    json_value["packet_id"] = serde_json::json!(format!("0x{:02x}", packet_id));
                }
                
                if !decoded.fields.is_empty() {
                    json_value["decoded_fields"] = serde_json::Value::Object(
                        decoded.fields.into_iter().map(|(k, v)| (k, v)).collect()
                    );
                }
                
                // Always include raw data for now
                json_value["data"] = serde_json::json!(packet.data);
            } else {
                // No parser available, just show raw data
                json_value["data"] = serde_json::json!(packet.data);
            }
            
            match serde_json::to_string_pretty(&json_value) {
                Ok(json_str) => json_str,
                Err(e) => format!("Error formatting JSON: {}", e)
            }
        };

        // Split content into lines for scrolling
        let lines: Vec<&str> = details.lines().collect();
        let max_lines = chunks[2].height.saturating_sub(2) as usize; // Account for border
        let total_lines = lines.len();
        
        // Calculate scroll bounds
        let max_scroll = if total_lines > max_lines {
            (total_lines - max_lines) as u16
        } else {
            0
        };
        
        // Clamp scroll to valid range and update stored value
        // This ensures that if the user scrolled beyond max, we clamp it back
        // so they can scroll up properly
        if app.packet_details_scroll > max_scroll {
            app.packet_details_scroll = max_scroll;
        }
        let scroll = app.packet_details_scroll;
        
        // Extract visible lines
        let start_line = scroll as usize;
        let end_line = (start_line + max_lines).min(total_lines);
        let visible_content = if start_line < total_lines {
            lines[start_line..end_line].join("\n")
        } else {
            String::new()
        };
        
        let details_paragraph = Paragraph::new(visible_content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled(
                        format!(
                            "Packet Details ({}) {}",
                            if app.show_hex { "Hex" } else { "JSON" },
                            if max_scroll > 0 {
                                format!("[{}/{} lines]", scroll + 1, total_lines)
                            } else {
                                String::new()
                            }
                        ),
                        Style::default().fg(direction_color),
                    )),
            )
            .wrap(Wrap { trim: false });

        f.render_widget(details_paragraph, chunks[2]);
    } else {
        let empty = Paragraph::new("No packet selected")
            .block(Block::default().borders(Borders::ALL).title("Packet Details"));
        f.render_widget(empty, chunks[2]);
    }
}

fn render_timeline(f: &mut Frame, area: Rect, app: &ViewerApp) {
    let _log = match &app.current_log {
        Some(log) => log,
        None => return,
    };

    if app.current_log.as_ref().map(|l| l.packets.is_empty()).unwrap_or(true) {
        return;
    }
    
    let log = app.current_log.as_ref().unwrap();

    // Show a timeline around the current packet
    let window_size = (area.width as usize).saturating_sub(4).min(100);
    let current_idx = app.packet_index;
    let total = log.packets.len();

    // Calculate window start/end
    let half_window = window_size / 2;
    let start = current_idx.saturating_sub(half_window);
    let end = (start + window_size).min(total);

    let mut timeline_chars = Vec::new();
    let mut timeline_styles = Vec::new();

    for i in start..end {
        let direction = log.packets[i].direction;
        let (symbol, color) = match direction {
            PacketDirection::Clientbound => ('?', Color::Green),
            PacketDirection::Serverbound => ('?', Color::Blue),
        };

        let style = if i == current_idx {
            Style::default().fg(color).add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default().fg(color)
        };

        timeline_chars.push(symbol);
        timeline_styles.push(style);
    }

    // Create spans for the timeline
    let spans: Vec<Span> = timeline_chars
        .iter()
        .zip(timeline_styles.iter())
        .map(|(ch, style)| Span::styled(ch.to_string(), *style))
        .collect();

    let timeline = Paragraph::new(Line::from(spans))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Timeline (showing {}-{})", start + 1, end)),
        );

    f.render_widget(timeline, area);
}

fn hex_dump(data: &[u8], bytes_per_line: usize) -> String {
    let mut output = String::new();
    let mut offset = 0;
    
    for chunk in data.chunks(bytes_per_line) {
        // Hex bytes
        let hex: String = chunk
            .iter()
            .map(|b| format!("{:02x} ", b))
            .collect::<String>();
        
        // Pad hex to fixed width
        let hex_padded = format!("{:<48}", hex);
        
        // ASCII representation
        let ascii: String = chunk
            .iter()
            .map(|b| {
                if (32..127).contains(b) {
                    *b as char
                } else {
                    '.'
                }
            })
            .collect();

        output.push_str(&format!("{:04x}  {} {}\n", offset, hex_padded, ascii));
        offset += chunk.len();
    }
    output
}
