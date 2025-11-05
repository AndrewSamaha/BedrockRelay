mod packet_logger;

use anyhow::Result;
use bincode::deserialize;
use flate2::read::GzDecoder;
use packet_logger::PacketEntry;
use serde_json;
use std::fs;
use std::io::{Cursor, Read};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <log_file> [max_packets]", args[0]);
        eprintln!("Example: {} logs/session-id.bin.gz 10", args[0]);
        std::process::exit(1);
    }

    let log_path = PathBuf::from(&args[1]);
    let max_packets = args
        .get(2)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(10);

    if !log_path.exists() {
        eprintln!("Error: File not found: {}", log_path.display());
        std::process::exit(1);
    }

    println!("Reading log file: {}", log_path.display());
    println!("Max packets to read: {}\n", max_packets);

    // Read and decompress if needed
    let data = if log_path
        .extension()
        .and_then(|s| s.to_str())
        == Some("gz")
    {
        println!("Decompressing gzip file...");
        let file = fs::File::open(&log_path)
            .map_err(|e| anyhow::anyhow!("Failed to open file: {}", e))?;
        let mut decoder = GzDecoder::new(file);
        let mut buffer = Vec::new();
        
        // Try to read as much as possible even if decompression fails
        match decoder.read_to_end(&mut buffer) {
            Ok(_) => {
                println!("Successfully decompressed");
            }
            Err(e) => {
                eprintln!("Warning: Decompression error (file may be incomplete): {}", e);
                eprintln!("Read {} bytes before error", buffer.len());
                if buffer.is_empty() {
                    return Err(anyhow::anyhow!("Could not decompress any data: {}", e));
                }
                println!("Continuing with partial data...");
            }
        }
        println!("Decompressed size: {} bytes", buffer.len());
        if buffer.is_empty() {
            return Err(anyhow::anyhow!("Decompressed file is empty"));
        }
        println!("First 16 bytes (hex): {}", 
                 buffer.iter().take(16)
                     .map(|b| format!("{:02x}", b))
                     .collect::<Vec<_>>().join(" "));
        println!();
        buffer
    } else {
        let data = fs::read(&log_path)?;
        println!("File size: {} bytes", data.len());
        if data.is_empty() {
            return Err(anyhow::anyhow!("File is empty"));
        }
        println!("First 16 bytes (hex): {}", 
                 data.iter().take(16)
                     .map(|b| format!("{:02x}", b))
                     .collect::<Vec<_>>().join(" "));
        println!();
        data
    };

    // Try reading with new format (length prefix)
    println!("Attempting to read with NEW format (length-prefixed entries)...");
    let mut cursor = Cursor::new(&data);
    let mut packets = Vec::new();
    let mut attempts = 0;

    loop {
        if packets.len() >= max_packets {
            break;
        }

        let position = cursor.position() as usize;
        if data.len().saturating_sub(position) < 4 {
            println!("  Not enough data for length prefix (need 4 bytes, have {})", 
                     data.len().saturating_sub(position));
            break;
        }

        // Read length prefix
        let mut len_bytes = [0u8; 4];
        if cursor.read_exact(&mut len_bytes).is_err() {
            println!("  Failed to read length prefix");
            break;
        }

        let entry_len = u32::from_le_bytes(len_bytes) as usize;
        let current_position = cursor.position() as usize;
        let remaining = data.len().saturating_sub(current_position);

        println!("  Entry #{}: position={}, length_prefix={}, remaining={}", 
                 packets.len() + 1, position, entry_len, remaining);

        if entry_len == 0 || entry_len > 10_000_000 || entry_len > remaining {
            println!("  Invalid length prefix (len={}, remaining={}), trying old format...", 
                     entry_len, remaining);
            cursor.set_position(position as u64);
            break;
        }

        // Read the entry data
        let mut entry_data = vec![0u8; entry_len];
        if cursor.read_exact(&mut entry_data).is_err() {
            println!("  Failed to read entry data (needed {} bytes, have {})", 
                     entry_len, remaining);
            cursor.set_position(position as u64);
            break;
        }

        // Deserialize
        match deserialize::<PacketEntry>(&entry_data) {
            Ok(entry) => {
                packets.push(entry);
                println!("  ? Successfully read packet #{}", packets.len());
            }
            Err(e) => {
                println!("  ? Deserialization failed: {}", e);
                if packets.is_empty() {
                    cursor.set_position(position as u64);
                    break;
                } else {
                    // We've read some packets, stop here
                    break;
                }
            }
        }

        attempts += 1;
        if attempts > 1000 {
            println!("  Too many attempts, stopping");
            break;
        }
    }

    // If no packets read with new format, try old format
    if packets.is_empty() {
        println!("\nAttempting to read with OLD format (no length prefix)...");
        cursor.set_position(0);

        loop {
            if packets.len() >= max_packets {
                break;
            }

            let pos_before = cursor.position() as usize;
            if pos_before >= data.len() {
                break;
            }

            println!("  Entry #{}: position={}, remaining={}", 
                     packets.len() + 1, pos_before, data.len() - pos_before);

            match bincode::deserialize_from::<_, PacketEntry>(&mut cursor) {
                Ok(entry) => {
                    let pos_after = cursor.position() as usize;
                    packets.push(entry);
                    println!("  ? Successfully read packet #{} (read {} bytes)", 
                             packets.len(), pos_after - pos_before);
                    
                    if pos_after >= data.len() {
                        break;
                    }
                }
                Err(e) => {
                    println!("  ? Deserialization failed: {}", e);
                    cursor.set_position(pos_before as u64);
                    break;
                }
            }

            attempts += 1;
            if attempts > 1000 {
                println!("  Too many attempts, stopping");
                break;
            }
        }
    }

    println!("\n=== Results ===");
    println!("Successfully read {} packet(s)\n", packets.len());

    if packets.is_empty() {
        eprintln!("ERROR: Could not read any packets!");
        eprintln!("File size: {} bytes", data.len());
        eprintln!("First 32 bytes (hex): {}", 
                 data.iter().take(32)
                     .map(|b| format!("{:02x}", b))
                     .collect::<Vec<_>>().join(" "));
        std::process::exit(1);
    }

    // Output packets as JSON
    let output = serde_json::to_string_pretty(&packets)?;
    println!("{}", output);

    Ok(())
}
