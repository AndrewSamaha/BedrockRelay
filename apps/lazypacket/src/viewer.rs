mod packet_logger;
mod protocol;
mod db;

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
use std::io;
use db::{Database, Session as DbSession};

struct SessionLog {
    session_id: i32,
    packets: Vec<PacketEntry>,
    start_time: i64,
    protocol_version: Option<String>,
}

impl SessionLog {
    async fn load(db: &Database, session_id: i32) -> Result<Self> {
        let db_packets = db.get_packets(session_id).await?;

        if db_packets.is_empty() {
            return Err(anyhow::anyhow!("No packets found for session {}", session_id));
        }

        let mut packets = Vec::new();
        let mut start_time = None;
        let mut protocol_version = None;

        for db_packet in db_packets {
            // Convert database packet to PacketEntry
            let direction = match db_packet.direction.as_str() {
                "clientbound" => PacketDirection::Clientbound,
                "serverbound" => PacketDirection::Serverbound,
                _ => {
                    return Err(anyhow::anyhow!("Invalid direction: {}", db_packet.direction));
                }
            };

            // Convert timestamp to milliseconds since epoch
            let timestamp_ms = db_packet.ts.timestamp_millis();

            // Track start time (first packet's timestamp)
            if start_time.is_none() {
                start_time = Some(timestamp_ms);
            }

            // Extract protocol version from first packet
            if protocol_version.is_none() {
                protocol_version = Some(db_packet.server_version.clone());
            }

            // Store the JSON packet directly for display
            // Also serialize to bytes for compatibility with hex view and protocol parsing
            let data = serde_json::to_vec(&db_packet.packet)
                .context("Failed to serialize packet to JSON")?;

            packets.push(PacketEntry {
                timestamp: timestamp_ms,
                direction,
                data,
                protocol_version: Some(db_packet.server_version),
                packet_json: Some(db_packet.packet),
            });
        }

        Ok(Self {
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
    db: Database,
    sessions: Vec<(DbSession, usize)>, // session, packet_count
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
    async fn new() -> Result<Self> {
        let db = Database::connect().await?;
        
        // Load sessions from database
        let db_sessions = db.get_sessions().await?;
        let mut sessions = Vec::new();
        
        for session in db_sessions {
            let packet_count = db.get_session_packet_count(session.id).await?;
            sessions.push((session, packet_count));
        }

        // Try to load protocol parser for default version
        let protocol_parser = protocol::ProtocolParser::new("1.21.111").ok();
        
        Ok(Self {
            db,
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

    async fn load_session(&mut self) -> Result<()> {
        if let Some((session, _)) = self.sessions.get(self.selected_session) {
            let log = SessionLog::load(&self.db, session.id).await?;
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
        if self.packet_index > 0 {
            self.packet_index -= 1;
            // Reset scroll when packet changes
            self.packet_details_scroll = 0;
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

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file - try multiple locations
    // 1. Current working directory
    // 2. Two levels up (project root when running from apps/lazypacket/)
    // 3. One level up (when running from project root)
    let env_locations = [
        ".env",
        "../../.env",
        "../.env",
    ];
    
    let mut loaded = false;
    for location in &env_locations {
        let path = std::path::Path::new(location);
        if path.exists() {
            if let Ok(_) = dotenv::from_path(path) {
                loaded = true;
                break;
            }
        }
    }
    
    // Also try dotenv's default behavior (current dir)
    if !loaded {
        dotenv::dotenv().ok();
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?; // Clear the screen before drawing

    let mut app = ViewerApp::new().await?;
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
                                    if let Err(e) = app.load_session().await {
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
        .map(|(session, packet_count)| {
            let duration = if let Some(ended_at) = session.ended_at {
                let duration = ended_at - session.started_at;
                format!("{} packets | {}s", packet_count, duration.num_seconds())
            } else {
                format!("{} packets | Active", packet_count)
            };
            let text = format!(
                "Session #{} | Started: {} | {}",
                session.id,
                session.started_at.format("%Y-%m-%d %H:%M:%S"),
                duration
            );
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
        "Session: #{} | {} | Packet: {}/{} | Time: {} | View: {} | [?/?/h/l: navigate, ?/?/k/j: scroll details, PgUp/PgDn: jump 10, Home/End: first/last, x: view, q: back]",
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
            // JSON view (default) - display packet JSON from database
            if let Some(ref packet_json) = packet.packet_json {
                // If we have JSON packet from database, display it directly
                // The packet JSON already contains the packet structure
                match serde_json::to_string_pretty(packet_json) {
                    Ok(json_str) => {
                        // Add metadata header
                        format!(
                            "Direction: {}\nTimestamp: {}\nRelative Time: {:.3}s\n\nPacket JSON:\n{}",
                            direction_str,
                            time_str,
                            log.relative_time(packet.timestamp) as f64 / 1000.0,
                            json_str
                        )
                    },
                    Err(e) => format!("Error formatting JSON: {}", e)
                }
            } else {
                // Fallback: if no JSON packet available (e.g., from binary logs), show metadata and try to decode
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
                }
                
                // Include raw data as array for binary format
                json_value["data"] = serde_json::json!(packet.data);
                
                match serde_json::to_string_pretty(&json_value) {
                    Ok(json_str) => json_str,
                    Err(e) => format!("Error formatting JSON: {}", e)
                }
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
