// CLI utility to dump decoded packets from a log file
// Usage: packet_dump <log_file> [--count N]

mod packet_logger;
mod protocol;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use packet_logger::{PacketDirection, PacketEntry};
use serde_json;
use std::env;
use std::path::PathBuf;

struct SessionLog {
    path: PathBuf,
    session_id: uuid::Uuid,
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
        let session_id = uuid::Uuid::parse_str(filename)
            .context("Failed to parse session ID from filename")?;

        // Read log file (uncompressed)
        let data = std::fs::read(&path)
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
                Err(e) => {
                    // Deserialization failed - this might be old format
                    // If we successfully read some packets, this file is probably mixed format or corrupted
                    if !packets.is_empty() {
                        // We've read some packets successfully, stop here
                        eprintln!("Warning: Failed to deserialize packet after successfully reading {} packets: {}", packets.len(), e);
                        break;
                    }
                    // No packets read yet - reset and try old format
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
                            eprintln!("Warning: Failed to deserialize packet after successfully reading {} packets: {}", packets.len(), e);
                            break;
                        }
                        
                        // If we haven't read anything yet, this might be old format
                        // Just continue - we'll check at the end if we got any packets
                        break;
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
            
            // Show first few bytes for debugging and suggest the file might be corrupted or in an unsupported format
            let preview = data.iter().take(16).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");
            return Err(anyhow::anyhow!(
                "No packets found in log file. File size: {} bytes. First 16 bytes (hex): {}\n\
                 This could mean the file is corrupted, in an unsupported format, or was created with a different version of the proxy.",
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

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Usage: {} <log_file> [--count N]", args[0]);
        eprintln!("  log_file: Path to the log file to read");
        eprintln!("  --count N: Number of packets to dump (default: 10)");
        std::process::exit(1);
    }
    
    let log_file = PathBuf::from(&args[1]);
    
    // Parse count argument
    let mut count = 10; // Default
    for i in 2..args.len() {
        if args[i] == "--count" || args[i] == "-n" {
            if i + 1 < args.len() {
                count = args[i + 1].parse()
                    .context("Invalid count value. Must be a positive integer.")?;
            }
        }
    }
    
    // Load the log file
    let session_log = SessionLog::load(log_file)?;
    
    println!("Loaded log file: {}", session_log.path.display());
    println!("Session ID: {}", session_log.session_id);
    println!("Total packets: {}", session_log.packets.len());
    if let Some(ref version) = session_log.protocol_version {
        println!("Protocol version: {}", version);
    }
    println!();
    
    // Load protocol parser (optional - continue without it if loading fails)
    let protocol_version = session_log.protocol_version.as_deref().unwrap_or(protocol::PROTOCOL_VERSION);
    let parser = match protocol::ProtocolParser::new(protocol_version) {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("Warning: Failed to load protocol parser for version {}: {}", 
                     protocol_version, e);
            eprintln!("Packets will be shown without field decoding.");
            eprintln!();
            None
        }
    };
    
    // Decode and output first N packets
    let packets_to_show = count.min(session_log.packets.len());
    
    let mut output_packets = Vec::new();
    
    for (idx, packet) in session_log.packets.iter().take(packets_to_show).enumerate() {
        let direction_str = match packet.direction {
            PacketDirection::Clientbound => "? Clientbound",
            PacketDirection::Serverbound => "? Serverbound",
        };
        
        let timestamp_dt = DateTime::<Utc>::from_timestamp_millis(packet.timestamp)
            .unwrap_or_default();
        let time_str = timestamp_dt.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string();
        let relative_time_ms = session_log.relative_time(packet.timestamp);
        
        // Decode packet if parser is available
        let decoded = if let Some(ref p) = parser {
            p.decode_packet(&packet.data, packet.direction)
        } else {
            protocol::DecodedPacket {
                packet_id: None,
                packet_name: None,
                fields: std::collections::HashMap::new(),
            }
        };
        
        let mut packet_json = serde_json::json!({
            "index": idx,
            "direction": direction_str,
            "timestamp": packet.timestamp,
            "timestamp_formatted": time_str,
            "relative_time_ms": relative_time_ms,
            "size_bytes": packet.data.len(),
        });
        
        if let Some(packet_name) = decoded.packet_name {
            packet_json["packet_name"] = serde_json::json!(packet_name);
        }
        if let Some(packet_id) = decoded.packet_id {
            packet_json["packet_id"] = serde_json::json!(format!("0x{:02x}", packet_id));
        }
        
        if !decoded.fields.is_empty() {
            packet_json["decoded_fields"] = serde_json::Value::Object(
                decoded.fields.into_iter().collect()
            );
        }
        
        // Always include raw data (as base64 for compactness, or hex)
        let hex_data: Vec<String> = packet.data.iter().take(256).map(|b| format!("{:02x}", b)).collect();
        let data_preview = if packet.data.len() > 256 {
            format!("{}... (truncated, {} total bytes)", hex_data.join(""), packet.data.len())
        } else {
            hex_data.join("")
        };
        packet_json["data_hex"] = serde_json::json!(data_preview);
        
        output_packets.push(packet_json);
    }
    
    // Output as pretty JSON
    let output = serde_json::json!({
        "session_id": session_log.session_id.to_string(),
        "protocol_version": session_log.protocol_version,
        "total_packets": session_log.packets.len(),
        "packets_shown": packets_to_show,
        "packets": output_packets,
    });
    
    println!("{}", serde_json::to_string_pretty(&output)?);
    
    Ok(())
}