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
use std::collections::{BTreeMap, BTreeSet};
use db::{Database, Session as DbSession, DbPacketFilterSet, DbPacketFilter};

struct SessionLog {
    session_id: i32,
    packets: Vec<PacketEntry>,
    start_time: i64,
    protocol_version: Option<String>,
}

impl PacketFilterSet {
    fn to_db_filter_set(&self) -> DbPacketFilterSet {
        DbPacketFilterSet {
            filters: self.filters.iter().map(|f| {
                DbPacketFilter {
                    direction: f.direction.map(|d| match d {
                        FilterPacketDirection::Clientbound => "clientbound".to_string(),
                        FilterPacketDirection::Serverbound => "serverbound".to_string(),
                    }),
                    packet_name: f.packet_name.clone(),
                    packet_name_is_wildcard: f.packet_name_is_wildcard,
                }
            }).collect(),
        }
    }
    
    fn to_string(&self) -> String {
        self.filters.iter().map(|f| {
            let dir_str = match f.direction {
                Some(FilterPacketDirection::Clientbound) => "c",
                Some(FilterPacketDirection::Serverbound) => "s",
                None => "a",
            };
            if let Some(ref name) = f.packet_name {
                format!("{}.{}", dir_str, name)
            } else {
                dir_str.to_string()
            }
        }).collect::<Vec<_>>().join(",")
    }
}

impl SessionLog {
    async fn load(db: &Database, session_id: i32, filter: Option<PacketFilterSet>) -> Result<Self> {
        let db_filter_set = filter.as_ref().map(|f| f.to_db_filter_set());
        let db_packets = db.get_packets(session_id, db_filter_set.as_ref()).await?;

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
                packet_number: Some(db_packet.packet_number),
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

enum ConfirmationAction {
    DeleteTag { session_id: i32, tag: String },
    // Add more action types as needed
}

struct ViewerApp {
    db: Database,
    sessions: Vec<(DbSession, usize, Vec<String>)>, // session, packet_count, tags
    selected_session: usize,
    current_log: Option<SessionLog>,
    packet_index: usize,
    mode: ViewerMode,
    error_message: Option<String>,
    show_hex: bool, // Toggle between JSON (default) and hex view
    packet_details_scroll: u16, // Scroll offset for packet details panel
    diff_panel_scroll: u16, // Scroll offset for differences panel (compare mode)
    protocol_parser: Option<protocol::ProtocolParser>, // Loaded protocol parser
    filter_input: String, // Current filter input text
    current_filter: Option<PacketFilterSet>, // Currently applied filter
    is_loading: bool, // Whether we're currently loading packets
    loading_frame: u8, // Frame counter for loading animation
    compare_mode: bool, // Whether compare mode is active
    baseline_packet_index: Option<usize>, // Index of baseline packet for comparison
    baseline_packet_json: Option<serde_json::Value>, // JSON of baseline packet
    tag_input: String, // Current tag input text
    tag_management: Option<TagManagementState>, // Tag management modal state
    confirmation_dialog: Option<ConfirmationDialogState>, // Confirmation dialog state
}

struct TagManagementState {
    session_id: i32,
    tags: Vec<String>,
    selected_tag_index: usize,
    add_tag_mode: bool, // Whether we're in add tag input mode
}

struct ConfirmationDialogState {
    message: String,
    action: ConfirmationAction,
}

enum ViewerMode {
    SessionList,
    PacketView,
    FilterInput,
    TagManagement,
    ConfirmationDialog,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PacketFilter {
    direction: Option<FilterPacketDirection>, // None means "all directions"
    packet_name: Option<String>, // None means "all packet types"
    packet_name_is_wildcard: bool, // If true, packet_name contains wildcards (*)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterPacketDirection {
    Clientbound,
    Serverbound,
}

#[derive(Debug, Clone)]
struct PacketFilterSet {
    filters: Vec<PacketFilter>, // OR logic: packet matches if it matches any filter
}

impl ViewerApp {
    async fn new() -> Result<Self> {
        let db = Database::connect().await?;
        
        // Load sessions from database
        let db_sessions = db.get_sessions().await?;
        let mut sessions = Vec::new();
        
        for session in db_sessions {
            let packet_count = db.get_session_packet_count(session.id).await?;
            let tags = db.get_session_tags(session.id).await.unwrap_or_default();
            sessions.push((session, packet_count, tags));
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
            diff_panel_scroll: 0,
            protocol_parser,
            filter_input: String::new(),
            current_filter: None,
            is_loading: false,
            loading_frame: 0,
            compare_mode: false,
            baseline_packet_index: None,
            baseline_packet_json: None,
            tag_input: String::new(),
            tag_management: None,
            confirmation_dialog: None,
        })
    }

    async fn refresh_session_tags(&mut self, session_id: i32) -> Result<()> {
        let tags = self.db.get_session_tags(session_id).await?;
        // Update tags for the session in our sessions list
        if let Some((_, _, session_tags)) = self.sessions.iter_mut()
            .find(|(s, _, _)| s.id == session_id) {
            *session_tags = tags;
        }
        Ok(())
    }

    async fn load_session(&mut self) -> Result<()> {
        if let Some((session, _, _)) = self.sessions.get(self.selected_session) {
            self.is_loading = true;
            let filter = self.current_filter.clone();
            let result = SessionLog::load(&self.db, session.id, filter).await;
            self.is_loading = false;
            
            match result {
                Ok(log) => {
                    self.current_log = Some(log);
                    self.packet_index = 0;
                    self.packet_details_scroll = 0;
                    self.diff_panel_scroll = 0;
                    // Reset compare mode when loading new session
                    self.compare_mode = false;
                    self.baseline_packet_index = None;
                    self.baseline_packet_json = None;
                    // Initialize filter input to show current filter
                    self.filter_input = self.current_filter.as_ref()
                        .map(|f| f.to_string())
                        .unwrap_or_else(|| "a".to_string());
                    self.mode = ViewerMode::PacketView;
                    Ok(())
                }
                Err(e) => Err(e)
            }
        } else {
            Err(anyhow::anyhow!("No session selected"))
        }
    }
    
    fn parse_filter(input: &str) -> Option<PacketFilterSet> {
        let input = input.trim();
        if input.is_empty() {
            return None;
        }
        
        // Split by comma to handle multiple filters
        let filter_strings: Vec<&str> = input.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        
        if filter_strings.is_empty() {
            return None;
        }
        
        let mut filters = Vec::new();
        
        for filter_str in filter_strings {
            // Parse format: [direction][.packet_name]
            // direction: c (clientbound), s (serverbound), a (all), or empty (all)
            // packet_name: optional, delimited by period
            // packet_name can contain * for wildcard matching
            
            let filter_str = filter_str.trim();
            if filter_str.is_empty() {
                continue;
            }
            
            let (direction_char, packet_name) = if let Some(dot_pos) = filter_str.find('.') {
                let dir = &filter_str[..dot_pos];
                let name = &filter_str[dot_pos + 1..];
                (dir, Some(name.to_string()))
            } else {
                (filter_str, None)
            };
            
            let direction = match direction_char.to_lowercase().as_str() {
                "c" => Some(FilterPacketDirection::Clientbound),
                "s" => Some(FilterPacketDirection::Serverbound),
                "a" | "" => None, // "a" or empty means all directions
                _ => {
                    // Invalid direction - skip this filter
                    continue;
                }
            };
            
            // Check if packet_name contains wildcards (*)
            let packet_name_is_wildcard = packet_name.as_ref()
                .map(|name| name.contains('*'))
                .unwrap_or(false);
            
            filters.push(PacketFilter {
                direction,
                packet_name,
                packet_name_is_wildcard,
            });
        }
        
        if filters.is_empty() {
            None
        } else {
            Some(PacketFilterSet { filters })
        }
    }

    fn current_packet(&self) -> Option<&PacketEntry> {
        self.current_log.as_ref()?.packets.get(self.packet_index)
    }
    
    fn find_closest_packet_index(&self, target_packet_number: i64) -> usize {
        if let Some(log) = &self.current_log {
            if log.packets.is_empty() {
                return 0;
            }
            
            // Find the packet with the closest packet_number
            let mut closest_index = 0;
            let mut min_diff = i64::MAX;
            
            for (index, packet) in log.packets.iter().enumerate() {
                if let Some(packet_num) = packet.packet_number {
                    let diff = (packet_num - target_packet_number).abs();
                    if diff < min_diff {
                        min_diff = diff;
                        closest_index = index;
                    }
                }
            }
            
            closest_index
        } else {
            0
        }
    }

    fn prev_packet(&mut self) {
        if self.packet_index > 0 {
            self.packet_index -= 1;
            // Reset scroll when packet changes
            self.packet_details_scroll = 0;
            self.diff_panel_scroll = 0;
        }
    }

    fn next_packet(&mut self) {
        if let Some(log) = &self.current_log {
            if self.packet_index < log.packets.len().saturating_sub(1) {
                self.packet_index += 1;
                // Reset scroll when packet changes
                self.packet_details_scroll = 0;
                self.diff_panel_scroll = 0;
            }
        }
    }
}

#[derive(Debug, Clone)]
enum JsonDiff {
    Added(serde_json::Value),
    Removed(serde_json::Value),
    Modified {
        old: serde_json::Value,
        new: serde_json::Value,
    },
    Unchanged(serde_json::Value),
    ObjectDiff(BTreeMap<String, JsonDiff>),
    ArrayDiff(Vec<JsonDiff>),
}

fn compare_json(baseline: &serde_json::Value, current: &serde_json::Value) -> JsonDiff {
    match (baseline, current) {
        // Both are objects - compare keys
        (serde_json::Value::Object(baseline_obj), serde_json::Value::Object(current_obj)) => {
            let mut diff_map = BTreeMap::new();
            let mut all_keys: BTreeSet<&String> = baseline_obj.keys().collect();
            all_keys.extend(current_obj.keys());
            
            for key in all_keys {
                match (baseline_obj.get(key), current_obj.get(key)) {
                    (Some(b_val), Some(c_val)) => {
                        if b_val == c_val {
                            // Values are identical - skip (will be hidden)
                        } else {
                            // Values differ - recursively compare
                            diff_map.insert(key.clone(), compare_json(b_val, c_val));
                        }
                    }
                    (Some(b_val), None) => {
                        // Key in baseline but not in current - removed
                        diff_map.insert(key.clone(), JsonDiff::Removed(b_val.clone()));
                    }
                    (None, Some(c_val)) => {
                        // Key in current but not in baseline - added
                        diff_map.insert(key.clone(), JsonDiff::Added(c_val.clone()));
                    }
                    (None, None) => unreachable!(),
                }
            }
            
            if diff_map.is_empty() {
                JsonDiff::Unchanged(serde_json::Value::Object(serde_json::Map::new()))
            } else {
                JsonDiff::ObjectDiff(diff_map)
            }
        }
        // Both are arrays - compare elements
        (serde_json::Value::Array(baseline_arr), serde_json::Value::Array(current_arr)) => {
            let mut diff_vec = Vec::new();
            let max_len = baseline_arr.len().max(current_arr.len());
            
            for i in 0..max_len {
                match (baseline_arr.get(i), current_arr.get(i)) {
                    (Some(b_val), Some(c_val)) => {
                        if b_val == c_val {
                            // Elements are identical - skip
                        } else {
                            diff_vec.push(compare_json(b_val, c_val));
                        }
                    }
                    (Some(b_val), None) => {
                        diff_vec.push(JsonDiff::Removed(b_val.clone()));
                    }
                    (None, Some(c_val)) => {
                        diff_vec.push(JsonDiff::Added(c_val.clone()));
                    }
                    (None, None) => unreachable!(),
                }
            }
            
            if diff_vec.is_empty() {
                JsonDiff::Unchanged(serde_json::Value::Array(Vec::new()))
            } else {
                JsonDiff::ArrayDiff(diff_vec)
            }
        }
        // Different types or primitive values
        _ => {
            if baseline == current {
                JsonDiff::Unchanged(baseline.clone())
            } else {
                JsonDiff::Modified {
                    old: baseline.clone(),
                    new: current.clone(),
                }
            }
        }
    }
}

fn format_json_diff(diff: &JsonDiff, path: &str, indent: usize) -> Vec<(String, Color)> {
    let indent_str = "  ".repeat(indent);
    let mut result = Vec::new();
    
    match diff {
        JsonDiff::Added(value) => {
            let json_str = serde_json::to_string_pretty(value)
                .unwrap_or_else(|_| format!("{:?}", value));
            let lines: Vec<&str> = json_str.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let prefix = if i == 0 {
                    if path.is_empty() {
                        format!("{}+ ", indent_str)
                    } else {
                        format!("{}+ {}: ", indent_str, path)
                    }
                } else {
                    format!("{}  + ", indent_str)
                };
                result.push((format!("{}{}", prefix, line), Color::Green));
            }
        }
        JsonDiff::Removed(value) => {
            let json_str = serde_json::to_string_pretty(value)
                .unwrap_or_else(|_| format!("{:?}", value));
            let lines: Vec<&str> = json_str.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let prefix = if i == 0 {
                    if path.is_empty() {
                        format!("{}- ", indent_str)
                    } else {
                        format!("{}- {}: ", indent_str, path)
                    }
                } else {
                    format!("{}  - ", indent_str)
                };
                result.push((format!("{}{}", prefix, line), Color::Red));
            }
        }
        JsonDiff::Modified { old, new } => {
            let old_str = serde_json::to_string_pretty(old)
                .unwrap_or_else(|_| format!("{:?}", old));
            let new_str = serde_json::to_string_pretty(new)
                .unwrap_or_else(|_| format!("{:?}", new));
            
            // Show old value
            let old_lines: Vec<&str> = old_str.lines().collect();
            for (i, line) in old_lines.iter().enumerate() {
                let prefix = if i == 0 {
                    if path.is_empty() {
                        format!("{}- ", indent_str)
                    } else {
                        format!("{}- {}: ", indent_str, path)
                    }
                } else {
                    format!("{}  - ", indent_str)
                };
                result.push((format!("{}{}", prefix, line), Color::Red));
            }
            
            // Show new value
            let new_lines: Vec<&str> = new_str.lines().collect();
            for (i, line) in new_lines.iter().enumerate() {
                let prefix = if i == 0 {
                    if path.is_empty() {
                        format!("{}+ ", indent_str)
                    } else {
                        format!("{}+ {}: ", indent_str, path)
                    }
                } else {
                    format!("{}  + ", indent_str)
                };
                result.push((format!("{}{}", prefix, line), Color::Green));
            }
        }
        JsonDiff::ObjectDiff(map) => {
            for (key, value_diff) in map {
                let new_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", path, key)
                };
                let mut sub_result = format_json_diff(value_diff, &new_path, indent);
                result.append(&mut sub_result);
            }
        }
        JsonDiff::ArrayDiff(arr) => {
            for (i, elem_diff) in arr.iter().enumerate() {
                let new_path = if path.is_empty() {
                    format!("[{}]", i)
                } else {
                    format!("{}[{}]", path, i)
                };
                let mut sub_result = format_json_diff(elem_diff, &new_path, indent);
                result.append(&mut sub_result);
            }
        }
        JsonDiff::Unchanged(_) => {
            // Skip unchanged values - they're hidden by default
        }
    }
    
    result
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file - find project root first
    
    dotenvy::dotenv().ok();

    // Quick sanity check:
    dbg!(std::env::current_dir()?);
    dbg!(std::env::var("PROXY_DESTINATION_ADDRESS")?);

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
                                KeyCode::Char('t') => {
                                    // Enter tag management modal
                                    if let Some((session, _, _)) = app.sessions.get(app.selected_session) {
                                        let tags = app.db.get_session_tags(session.id).await.unwrap_or_default();
                                        app.tag_management = Some(TagManagementState {
                                            session_id: session.id,
                                            tags,
                                            selected_tag_index: 0,
                                            add_tag_mode: false,
                                        });
                                        app.tag_input = String::new();
                                        app.mode = ViewerMode::TagManagement;
                                    }
                                }
                                _ => {}
                            }
                        }
                        ViewerMode::PacketView => {
                            match key.code {
                                KeyCode::Char('q') => {
                                    app.mode = ViewerMode::SessionList;
                                    app.current_log = None;
                                    // Reset compare mode when going back to session list
                                    app.compare_mode = false;
                                    app.baseline_packet_index = None;
                                    app.baseline_packet_json = None;
                                }
                                KeyCode::Esc => {
                                    // Exit compare mode if active, otherwise go back to session list
                                    if app.compare_mode {
                                        app.compare_mode = false;
                                        app.baseline_packet_index = None;
                                        app.baseline_packet_json = None;
                                        app.packet_details_scroll = 0;
                                        app.diff_panel_scroll = 0;
                                    } else {
                                        app.mode = ViewerMode::SessionList;
                                        app.current_log = None;
                                    }
                                }
                                KeyCode::Char('c') => {
                                    // Enter compare mode / Set baseline
                                    let packet_json_opt = app.current_packet()
                                        .and_then(|p| p.packet_json.as_ref())
                                        .map(|j| j.clone());
                                    if let Some(packet_json) = packet_json_opt {
                                        app.compare_mode = true;
                                        app.baseline_packet_index = Some(app.packet_index);
                                        app.baseline_packet_json = Some(packet_json);
                                        app.packet_details_scroll = 0;
                                        app.diff_panel_scroll = 0;
                                    }
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
                                        app.diff_panel_scroll = 0;
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
                                        app.diff_panel_scroll = 0;
                                    }
                                }
                                KeyCode::Home => {
                                    app.packet_index = 0;
                                    app.packet_details_scroll = 0;
                                    app.diff_panel_scroll = 0;
                                }
                                KeyCode::End => {
                                    if let Some(log) = &app.current_log {
                                        app.packet_index = log.packets.len().saturating_sub(1);
                                        app.packet_details_scroll = 0;
                                        app.diff_panel_scroll = 0;
                                    }
                                }
                                KeyCode::Char('x') | KeyCode::Char('X') => {
                                    // Toggle between JSON and hex view
                                    app.show_hex = !app.show_hex;
                                    // Reset scroll when toggling view
                                    app.packet_details_scroll = 0;
                                    app.diff_panel_scroll = 0;
                                }
                                KeyCode::Char('f') | KeyCode::Char('F') => {
                                    // Enter filter input mode
                                    // Initialize filter input with current filter if one exists
                                    app.filter_input = app.current_filter.as_ref()
                                        .map(|f| f.to_string())
                                        .unwrap_or_else(|| "a".to_string());
                                    app.mode = ViewerMode::FilterInput;
                                }
                                _ => {}
                            }
                        }
                        ViewerMode::FilterInput => {
                            match key.code {
                                KeyCode::Esc => {
                                    // Cancel filter - revert filter input to current filter
                                    app.filter_input = app.current_filter.as_ref()
                                        .map(|f| f.to_string())
                                        .unwrap_or_else(|| "a".to_string());
                                    app.mode = ViewerMode::PacketView;
                                }
                                KeyCode::Enter => {
                                    // Apply filter
                                    let filter = ViewerApp::parse_filter(&app.filter_input);
                                    
                                    // Save current packet number to preserve position
                                    let current_packet_number = app.current_packet()
                                        .and_then(|p| p.packet_number)
                                        .or_else(|| {
                                            app.current_log.as_ref()
                                                .and_then(|log| log.packets.first())
                                                .and_then(|p| p.packet_number)
                                        });
                                    
                                    app.current_filter = filter;
                                    // Keep filter_input visible so user can see what filter is applied
                                    app.mode = ViewerMode::PacketView;
                                    
                                    // Reload session with new filter
                                    if let Some((session, _, _)) = app.sessions.get(app.selected_session) {
                                        app.is_loading = true;
                                        let filter_to_apply = app.current_filter.clone();
                                        let result = SessionLog::load(&app.db, session.id, filter_to_apply).await;
                                        app.is_loading = false;
                                        
                                        match result {
                                            Ok(log) => {
                                                app.current_log = Some(log);
                                                
                                                // Reset compare mode when applying filter
                                                app.compare_mode = false;
                                                app.baseline_packet_index = None;
                                                app.baseline_packet_json = None;
                                                
                                                // Try to preserve packet position by finding closest packet_number
                                                if let Some(target_packet_num) = current_packet_number {
                                                    app.packet_index = app.find_closest_packet_index(target_packet_num);
                                                } else {
                                                    app.packet_index = 0;
                                                }
                                                
                                app.packet_details_scroll = 0;
                                app.diff_panel_scroll = 0;
                                // Keep filter_input showing the applied filter
                                            }
                                            Err(e) => {
                                                app.error_message = Some(format!("Failed to load filtered packets: {}", e));
                                            }
                                        }
                                    }
                                }
                                KeyCode::Backspace => {
                                    app.filter_input.pop();
                                }
                                KeyCode::Char(c) => {
                                    // Allow multi-character input for filter strings
                                    app.filter_input.push(c);
                                }
                                _ => {}
                            }
                        }
                        ViewerMode::TagManagement => {
                            if let Some(ref mut tag_mgmt) = app.tag_management {
                                if tag_mgmt.add_tag_mode {
                                    // In add tag input mode
                                    match key.code {
                                        KeyCode::Esc => {
                                            tag_mgmt.add_tag_mode = false;
                                            app.tag_input = String::new();
                                        }
                                        KeyCode::Enter => {
                                            let tag = app.tag_input.trim().to_string();
                                            if !tag.is_empty() {
                                                let session_id = tag_mgmt.session_id;
                                                let add_result = app.db.add_session_tag(session_id, &tag).await;
                                                
                                                // Drop mutable borrow of tag_mgmt before calling refresh_session_tags
                                                match add_result {
                                                    Ok(_) => {
                                                        // Refresh tags in tag management
                                                        if let Ok(updated_tags) = app.db.get_session_tags(session_id).await {
                                                            if let Some(ref mut tm) = app.tag_management {
                                                                tm.tags = updated_tags;
                                                                tm.add_tag_mode = false;
                                                            }
                                                        }
                                                        // Update session list too
                                                        if let Err(e) = app.refresh_session_tags(session_id).await {
                                                            app.error_message = Some(format!("Failed to refresh tags: {}", e));
                                                        }
                                                    }
                                                    Err(e) => {
                                                        app.error_message = Some(format!("Failed to add tag: {}", e));
                                                        if let Some(ref mut tm) = app.tag_management {
                                                            tm.add_tag_mode = false;
                                                        }
                                                    }
                                                }
                                            } else {
                                                tag_mgmt.add_tag_mode = false;
                                            }
                                            app.tag_input = String::new();
                                        }
                                        KeyCode::Backspace => {
                                            app.tag_input.pop();
                                        }
                                        KeyCode::Char(c) => {
                                            app.tag_input.push(c);
                                        }
                                        _ => {}
                                    }
                                } else {
                                    // In tag list mode
                                    match key.code {
                                        KeyCode::Esc | KeyCode::Char('q') => {
                                            app.tag_management = None;
                                            app.mode = ViewerMode::SessionList;
                                        }
                                        KeyCode::Up => {
                                            if tag_mgmt.selected_tag_index > 0 {
                                                tag_mgmt.selected_tag_index -= 1;
                                            }
                                        }
                                        KeyCode::Down => {
                                            if tag_mgmt.selected_tag_index < tag_mgmt.tags.len().saturating_sub(1) {
                                                tag_mgmt.selected_tag_index += 1;
                                            }
                                        }
                                        KeyCode::Char('d') => {
                                            // Delete selected tag
                                            if let Some(tag) = tag_mgmt.tags.get(tag_mgmt.selected_tag_index) {
                                                let tag_to_delete = tag.clone();
                                                app.confirmation_dialog = Some(ConfirmationDialogState {
                                                    message: format!("Delete tag '{}'?", tag_to_delete),
                                                    action: ConfirmationAction::DeleteTag {
                                                        session_id: tag_mgmt.session_id,
                                                        tag: tag_to_delete,
                                                    },
                                                });
                                                app.mode = ViewerMode::ConfirmationDialog;
                                            }
                                        }
                                        KeyCode::Char('a') => {
                                            // Enter add tag mode
                                            tag_mgmt.add_tag_mode = true;
                                            app.tag_input = String::new();
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        ViewerMode::ConfirmationDialog => {
                            match key.code {
                                KeyCode::Esc | KeyCode::Char('n') => {
                                    // Cancel confirmation
                                    app.confirmation_dialog = None;
                                    // Return to previous mode
                                    if app.tag_management.is_some() {
                                        app.mode = ViewerMode::TagManagement;
                                    } else {
                                        app.mode = ViewerMode::SessionList;
                                    }
                                }
                                KeyCode::Enter | KeyCode::Char('y') => {
                                    // Confirm action
                                    if let Some(dialog) = app.confirmation_dialog.take() {
                                        match dialog.action {
                                            ConfirmationAction::DeleteTag { session_id, tag } => {
                                                match app.db.remove_session_tag(session_id, &tag).await {
                                                    Ok(_) => {
                                                        // Refresh tags
                                                        if let Ok(updated_tags) = app.db.get_session_tags(session_id).await {
                                                            if let Some(ref mut tag_mgmt) = app.tag_management {
                                                                tag_mgmt.tags = updated_tags;
                                                                // Adjust selected index if needed
                                                                if tag_mgmt.selected_tag_index >= tag_mgmt.tags.len() && !tag_mgmt.tags.is_empty() {
                                                                    tag_mgmt.selected_tag_index = tag_mgmt.tags.len() - 1;
                                                                } else if tag_mgmt.tags.is_empty() {
                                                                    tag_mgmt.selected_tag_index = 0;
                                                                }
                                                            }
                                                            // Update session list too
                                                            if let Err(e) = app.refresh_session_tags(session_id).await {
                                                                app.error_message = Some(format!("Failed to refresh tags: {}", e));
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        app.error_message = Some(format!("Failed to delete tag: {}", e));
                                                    }
                                                }
                                                // Return to tag management
                                                if app.tag_management.is_some() {
                                                    app.mode = ViewerMode::TagManagement;
                                                } else {
                                                    app.mode = ViewerMode::SessionList;
                                                }
                                            }
                                        }
                                    }
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
    // Update loading animation frame
    if app.is_loading {
        app.loading_frame = app.loading_frame.wrapping_add(1);
    }
    
    match app.mode {
        ViewerMode::SessionList => render_session_list(f, app),
        ViewerMode::PacketView | ViewerMode::FilterInput => render_packet_view(f, app),
        ViewerMode::TagManagement => render_tag_management(f, app),
        ViewerMode::ConfirmationDialog => {
            // Render the underlying view first, then overlay the confirmation dialog
            match app.tag_management {
                Some(_) => render_tag_management(f, app),
                _ => render_session_list(f, app),
            }
            render_confirmation_dialog(f, app);
        }
    }
}

fn render_session_list(f: &mut Frame, app: &mut ViewerApp) {
    let show_error = app.error_message.is_some();
    
    let chunks = if show_error {
        Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(f.size())
    } else {
        Layout::default()
            .constraints([Constraint::Min(0)])
            .split(f.size())
    };
    
    let main_area = if show_error {
        // Show error message if present
        let error_paragraph = Paragraph::new(app.error_message.as_ref().unwrap().as_str())
            .block(Block::default().borders(Borders::ALL).title("Error").style(Style::default().fg(Color::Red)))
            .wrap(Wrap { trim: false });
        f.render_widget(error_paragraph, chunks[0]);
        chunks[1]
    } else {
        chunks[0]
    };

    let items: Vec<ListItem> = app
        .sessions
        .iter()
        .map(|(session, packet_count, tags)| {
            let duration = if let Some(ended_at) = session.ended_at {
                let duration = ended_at - session.started_at;
                format!("{} packets | {}s", packet_count, duration.num_seconds())
            } else {
                format!("{} packets | Active", packet_count)
            };
            let tags_str = if tags.is_empty() {
                String::new()
            } else {
                format!(" | Tags: {}", tags.join(", "))
            };
            let text = format!(
                "Session #{} | Started: {} | {}{}",
                session.id,
                session.started_at.format("%Y-%m-%d %H:%M:%S"),
                duration,
                tags_str
            );
            ListItem::new(text)
        })
        .collect();

    use ratatui::widgets::ListState;
    let mut list_state = ListState::default();
    list_state.select(Some(app.selected_session));

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Session Logs (↑↓ to navigate, Enter to select, t to tag, q to quit)"))
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
            Constraint::Length(6), // Filter panel (taller to fit longer help text)
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
    let filter_str = app.current_filter.as_ref()
        .map(|f| format!(" [Filter: {}]", f.to_string()))
        .unwrap_or_else(|| String::new());
    let compare_str = if app.compare_mode {
        format!(" [Compare Mode | Baseline: Packet {}]", 
            app.baseline_packet_index.map(|i| i + 1).unwrap_or(0))
    } else {
        String::new()
    };
    let version_str = log.protocol_version.as_ref()
        .map(|v| format!("Protocol: {}", v))
        .unwrap_or_else(|| "Protocol: Unknown".to_string());
    let header_text = format!(
        "Session: #{} | {} | Packet: {}/{} | Time: {} | View: {}{}{} | [Left/Right/h/l: navigate, Up/Down/k/j: scroll details, PgUp/PgDn: jump 10, Home/End: first/last, x: view, f: filter, c: compare, Esc: exit compare, q: back]",
        log.session_id,
        version_str,
        packet_num,
        total_packets,
        session_time,
        view_mode,
        filter_str,
        compare_str
    );

    let header = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::ALL).title("lazypacket"));
    f.render_widget(header, chunks[0]);

    // Filter panel
    render_filter_panel(f, chunks[1], &app);

    // Timeline visualization
    render_timeline(f, chunks[2], app);

    // Split packet details area horizontally if in compare mode
    let detail_chunks: Vec<Rect> = if app.compare_mode && !app.show_hex {
        Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[3])
            .to_vec()
    } else {
        // Single panel - use full width
        vec![chunks[3]]
    };

    // Extract packet data and scroll values before modifying app
    let packet_json_for_diff = packet.and_then(|p| p.packet_json.clone());
    let is_baseline_for_diff = app.baseline_packet_index == Some(app.packet_index);
    let baseline_json_for_diff = app.baseline_packet_json.clone();
    let diff_panel_scroll_value = app.diff_panel_scroll;
    
    // Extract metadata for delta calculation
    let current_packet_timestamp = packet.map(|p| p.timestamp);
    let current_packet_number = packet.and_then(|p| p.packet_number);
    let baseline_packet_timestamp = app.baseline_packet_index
        .and_then(|idx| log.packets.get(idx))
        .map(|p| p.timestamp);
    let baseline_packet_number = app.baseline_packet_index
        .and_then(|idx| log.packets.get(idx))
        .and_then(|p| p.packet_number);

    // Packet details (left panel, or full width if not in compare mode)
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

        let packet_number_str = packet.packet_number
            .map(|n| format!("Packet Number: {}\n", n))
            .unwrap_or_else(|| String::new());
        
        let details = if app.show_hex {
            // Hex view
            format!(
                "Direction: {}\nTimestamp: {}\n{}Size: {} bytes\n\nHex Dump:\n{}",
                direction_str,
                time_str,
                packet_number_str,
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
                            "Direction: {}\nTimestamp: {}\n{}Relative Time: {:.3}s\n\nPacket JSON:\n{}",
                            direction_str,
                            time_str,
                            packet_number_str,
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
                
                // Add packet_number if available
                if let Some(packet_num) = packet.packet_number {
                    json_value["packet_number"] = serde_json::json!(packet_num);
                }
                
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

        // Regular mode - plain text lines for packet details
        let lines: Vec<&str> = details.lines().collect();
        let lines_vec: Vec<Line> = lines.iter().map(|l| Line::from(*l)).collect();
        let total_lines = lines_vec.len();
        
        let max_lines = detail_chunks[0].height.saturating_sub(2) as usize; // Account for border
        
        // Calculate scroll bounds
        let max_scroll = if total_lines > max_lines {
            (total_lines - max_lines) as u16
        } else {
            0
        };
        
        // Clamp scroll to valid range and update stored value
        if app.packet_details_scroll > max_scroll {
            app.packet_details_scroll = max_scroll;
        }
        let scroll = app.packet_details_scroll;
        
        // Extract visible lines
        let start_line = scroll as usize;
        let end_line = (start_line + max_lines).min(total_lines);
        let visible_lines: Vec<Line> = if start_line < total_lines {
            lines_vec[start_line..end_line].to_vec()
        } else {
            Vec::new()
        };
        
        let details_paragraph = Paragraph::new(visible_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled(
                        format!(
                            "Packet Details ({}) {}",
                            if app.show_hex { "Hex" } else if app.compare_mode { "Compare" } else { "JSON" },
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

        f.render_widget(details_paragraph, detail_chunks[0]);

        // Render differences panel if in compare mode
        if app.compare_mode && !app.show_hex && detail_chunks.len() > 1 {
            render_diff_panel(
                f, 
                detail_chunks[1], 
                &packet_json_for_diff, 
                &baseline_json_for_diff, 
                is_baseline_for_diff,
                current_packet_timestamp,
                current_packet_number,
                baseline_packet_timestamp,
                baseline_packet_number,
                diff_panel_scroll_value, 
                &mut app.diff_panel_scroll
            );
        }
    } else {
        let empty = Paragraph::new("No packet selected")
            .block(Block::default().borders(Borders::ALL).title("Packet Details"));
        f.render_widget(empty, detail_chunks[0]);
        
        // Render empty diff panel if in compare mode
        if app.compare_mode && !app.show_hex && detail_chunks.len() > 1 {
            let empty_diff = Paragraph::new("No packet selected")
                .block(Block::default().borders(Borders::ALL).title("Differences"));
            f.render_widget(empty_diff, detail_chunks[1]);
        }
    }
    
    // Show loading indicator overlay if loading
    render_loading_indicator(f, app);
}

fn render_diff_panel(
    f: &mut Frame, 
    area: Rect, 
    packet_json: &Option<serde_json::Value>, 
    baseline_json: &Option<serde_json::Value>, 
    is_baseline: bool,
    current_timestamp: Option<i64>,
    current_packet_number: Option<i64>,
    baseline_timestamp: Option<i64>,
    baseline_packet_number: Option<i64>,
    scroll: u16, 
    scroll_ref: &mut u16
) {
    // Build colored lines for differences
    let (diff_lines_vec, total_diff_lines) = if let Some(ref packet_json) = packet_json {
        if let Some(ref baseline_json) = baseline_json {
            let mut all_lines = Vec::new();
            
            // Add metadata deltas at the top
            if !is_baseline {
                // Time delta
                if let (Some(current_ts), Some(baseline_ts)) = (current_timestamp, baseline_timestamp) {
                    let time_delta_ms = current_ts - baseline_ts;
                    let time_delta_sec = time_delta_ms as f64 / 1000.0;
                    let time_delta_str = if time_delta_ms >= 0 {
                        format!("Time delta: +{:.3}s", time_delta_sec)
                    } else {
                        format!("Time delta: {:.3}s", time_delta_sec)
                    };
                    all_lines.push(Line::from(Span::styled(
                        time_delta_str,
                        Style::default().fg(Color::Cyan)
                    )));
                }
                
                // Packet number delta
                if let (Some(current_num), Some(baseline_num)) = (current_packet_number, baseline_packet_number) {
                    let packet_delta = current_num - baseline_num;
                    let packet_delta_str = if packet_delta >= 0 {
                        format!("Packet number delta: +{}", packet_delta)
                    } else {
                        format!("Packet number delta: {}", packet_delta)
                    };
                    all_lines.push(Line::from(Span::styled(
                        packet_delta_str,
                        Style::default().fg(Color::Cyan)
                    )));
                }
                
                if (current_timestamp.is_some() && baseline_timestamp.is_some()) || 
                   (current_packet_number.is_some() && baseline_packet_number.is_some()) {
                    all_lines.push(Line::from(""));
                }
            }
            
            if is_baseline {
                all_lines.push(Line::from(Span::styled(
                    "This is the baseline packet for comparison.",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                )));
                all_lines.push(Line::from(""));
                all_lines.push(Line::from("Navigate to other packets to see differences."));
            } else {
                let diff = compare_json(baseline_json, packet_json);
                let diff_lines = format_json_diff(&diff, "", 0);
                
                if diff_lines.is_empty() {
                    all_lines.push(Line::from("No differences from baseline packet."));
                } else {
                    all_lines.push(Line::from("Differences from baseline:"));
                    all_lines.push(Line::from(""));
                    
                    // Add colored diff lines
                    for (line, color) in diff_lines {
                        all_lines.push(Line::from(Span::styled(line, Style::default().fg(color))));
                    }
                }
            }
            
            let total_lines = all_lines.len();
            (all_lines, total_lines)
        } else {
            (vec![Line::from("Error: Baseline packet JSON not available")], 1)
        }
    } else {
        (vec![Line::from("Error: Current packet JSON not available for comparison")], 1)
    };
    
    let max_lines = area.height.saturating_sub(2) as usize; // Account for border
    
    // Calculate scroll bounds for diff panel
    let max_scroll = if total_diff_lines > max_lines {
        (total_diff_lines - max_lines) as u16
    } else {
        0
    };
    
    // Clamp scroll to valid range and update the reference
    let mut clamped_scroll = scroll;
    if clamped_scroll > max_scroll {
        clamped_scroll = max_scroll;
    }
    *scroll_ref = clamped_scroll;
    
    // Extract visible lines using clamped scroll
    let start_line = clamped_scroll as usize;
    let end_line = (start_line + max_lines).min(total_diff_lines);
    let visible_lines: Vec<Line> = if start_line < total_diff_lines {
        diff_lines_vec[start_line..end_line].to_vec()
    } else {
        Vec::new()
    };
    
    let diff_paragraph = Paragraph::new(visible_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    format!(
                        "Differences {}",
                        if max_scroll > 0 {
                            format!("[{}/{} lines]", clamped_scroll + 1, total_diff_lines)
                        } else {
                            String::new()
                        }
                    ),
                    Style::default().fg(Color::Cyan),
                )),
        )
        .wrap(Wrap { trim: false });
    
    f.render_widget(diff_paragraph, area);
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

        let is_baseline = app.compare_mode && app.baseline_packet_index == Some(i);
        let is_current = i == current_idx;

        let style = if is_current && is_baseline {
            // Current packet is also baseline - use yellow with bold and reversed
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else if is_current {
            // Current packet (not baseline)
            Style::default().fg(color).add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else if is_baseline {
            // Baseline packet (not current) - use yellow background
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            // Regular packet
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

    let timeline_title = if app.compare_mode && app.baseline_packet_index.is_some() {
        format!("Timeline (showing {}-{}) | Baseline: Packet {}", 
            start + 1, end, app.baseline_packet_index.map(|i| i + 1).unwrap_or(0))
    } else {
        format!("Timeline (showing {}-{})", start + 1, end)
    };
    
    let timeline = Paragraph::new(Line::from(spans))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(timeline_title),
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

fn render_filter_panel(f: &mut Frame, area: Rect, app: &ViewerApp) {
    let filter_text = format!("Filter: {}", app.filter_input);
    let help_text = "Format: [c|s|a][.packet_name][,filter2,...] | Examples: s.player_auth_input, c.start_game, s.*action* | Enter to apply, Esc to cancel";
    
    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Input line (with border, needs 3 lines)
            Constraint::Length(3), // Help text (increased for longer text)
        ])
        .split(area);
    
    let input_style = if matches!(app.mode, ViewerMode::FilterInput) {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    
    let input_paragraph = Paragraph::new(filter_text.as_str())
        .block(Block::default().borders(Borders::ALL).title("Filter Packets"))
        .style(input_style);
    f.render_widget(input_paragraph, chunks[0]);
    
    let help_paragraph = Paragraph::new(help_text)
        .block(Block::default())
        .style(Style::default().fg(Color::DarkGray))
        .wrap(Wrap { trim: false });
    f.render_widget(help_paragraph, chunks[1]);
    
    // Show cursor only when in FilterInput mode
    if matches!(app.mode, ViewerMode::FilterInput) {
        f.set_cursor(
            chunks[0].x + 8 + app.filter_input.len() as u16,
            chunks[0].y + 1,
        );
    }
}

fn render_loading_indicator(f: &mut Frame, app: &ViewerApp) {
    if !app.is_loading {
        return;
    }
    
    // Create centered popup
    let popup_area = centered_rect(30, 5, f.size());
    
    // Animated loading spinner
    let spinner_chars = ['?', '?', '?', '?', '?', '?', '?', '?', '?', '?'];
    let spinner = spinner_chars[(app.loading_frame as usize / 3) % spinner_chars.len()];
    
    let loading_text = format!("{} Loading packets...", spinner);
    let loading_paragraph = Paragraph::new(loading_text)
        .block(Block::default().borders(Borders::ALL).title("Loading"))
        .style(Style::default().fg(Color::Cyan))
        .alignment(ratatui::layout::Alignment::Center);
    
    f.render_widget(loading_paragraph, popup_area);
}

fn render_tag_management(f: &mut Frame, app: &mut ViewerApp) {
    if let Some(ref mut tag_mgmt) = app.tag_management {
        // Create modal area (centered, 60% width, 70% height)
        let modal_area = centered_rect(60, 70, f.size());
        
        let chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Min(0),     // Tag list or add input
                Constraint::Length(3),  // Help text
            ])
            .split(modal_area);
        
        // Title
        let title = Paragraph::new(format!("Tags for Session #{}", tag_mgmt.session_id))
            .block(Block::default().borders(Borders::ALL).title("Tag Management"))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(title, chunks[0]);
        
        if tag_mgmt.add_tag_mode {
            // Add tag input mode
            let tag_text = format!("Tag: {}", app.tag_input);
            let input_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
            
            let input_paragraph = Paragraph::new(tag_text.as_str())
                .block(Block::default().borders(Borders::ALL).title("Add Tag"))
                .style(input_style);
            f.render_widget(input_paragraph, chunks[1]);
            
            // Show cursor
            f.set_cursor(
                chunks[1].x + 6 + app.tag_input.len() as u16,
                chunks[1].y + 1,
            );
            
            let help_text = "Enter tag name and press Enter to add, Esc to cancel";
            let help_paragraph = Paragraph::new(help_text)
                .block(Block::default())
                .style(Style::default().fg(Color::DarkGray))
                .wrap(Wrap { trim: false });
            f.render_widget(help_paragraph, chunks[2]);
        } else {
            // Tag list mode
            let items: Vec<ListItem> = if tag_mgmt.tags.is_empty() {
                vec![ListItem::new("(No tags)")]
            } else {
                tag_mgmt.tags
                    .iter()
                    .enumerate()
                    .map(|(idx, tag)| {
                        let text = if idx == tag_mgmt.selected_tag_index {
                            format!("> {}", tag)
                        } else {
                            format!("  {}", tag)
                        };
                        ListItem::new(text)
                    })
                    .collect()
            };
            
            use ratatui::widgets::ListState;
            let mut list_state = ListState::default();
            if !tag_mgmt.tags.is_empty() {
                list_state.select(Some(tag_mgmt.selected_tag_index));
            }
            
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title("Tags"))
                .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            
            f.render_stateful_widget(list, chunks[1], &mut list_state);
            
            let help_text = "↑↓: navigate | a: add tag | d: delete tag | Esc/q: close";
            let help_paragraph = Paragraph::new(help_text)
                .block(Block::default())
                .style(Style::default().fg(Color::DarkGray))
                .wrap(Wrap { trim: false });
            f.render_widget(help_paragraph, chunks[2]);
        }
    }
}

fn render_confirmation_dialog(f: &mut Frame, app: &mut ViewerApp) {
    if let Some(ref dialog) = app.confirmation_dialog {
        // Create centered dialog (40% width, 20% height)
        let dialog_area = centered_rect(40, 20, f.size());
        
        let chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                Constraint::Min(0),     // Message
                Constraint::Length(1),  // Buttons
            ])
            .split(dialog_area);
        
        // Message
        let message_lines: Vec<Line> = dialog.message
            .lines()
            .map(|l| Line::from(l))
            .collect();
        
        let message_paragraph = Paragraph::new(message_lines)
            .block(Block::default().borders(Borders::ALL).title("Confirm"))
            .wrap(Wrap { trim: false })
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(message_paragraph, chunks[0]);
        
        // Buttons
        let button_text = "[Y]es / [N]o (or Enter/Esc)";
        let button_paragraph = Paragraph::new(button_text)
            .block(Block::default())
            .style(Style::default().fg(Color::Cyan))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(button_paragraph, chunks[1]);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
