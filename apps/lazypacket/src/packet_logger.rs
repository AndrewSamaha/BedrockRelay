use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize};
use serde_json::Value;
use uuid::Uuid;
use chrono::Utc;

// Default protocol version - matches protocol.rs
const DEFAULT_PROTOCOL_VERSION: &str = "1.21.111";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketEntry {
    pub timestamp: i64,
    pub direction: PacketDirection,
    pub data: Vec<u8>,
    #[serde(default)]
    pub protocol_version: Option<String>,
    #[serde(skip)]
    pub packet_json: Option<Value>,
    #[serde(skip)]
    pub packet_number: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PacketDirection {
    Clientbound,
    Serverbound,
}

pub struct PacketLogger {
    session_id: Uuid,
    log_path: PathBuf,
    writer: Option<BufWriter<File>>,
    protocol_version: String,
}

impl PacketLogger {
    pub fn new(session_id: Uuid, log_dir: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        Self::with_protocol_version(session_id, log_dir, DEFAULT_PROTOCOL_VERSION.to_string())
    }

    pub fn with_protocol_version(
        session_id: Uuid, 
        log_dir: impl AsRef<Path>,
        protocol_version: String,
    ) -> Result<Self, std::io::Error> {
        let log_dir = log_dir.as_ref();
        
        // Create log directory if it doesn't exist
        std::fs::create_dir_all(log_dir)?;

        // Create log file path: logs/session_id.bin
        let log_path = log_dir.join(format!("{}.bin", session_id));

        let file = File::create(&log_path)?;
        let writer = BufWriter::new(file);

        Ok(Self {
            session_id,
            log_path,
            writer: Some(writer),
            protocol_version,
        })
    }

    pub fn log_packet(&mut self, direction: PacketDirection, data: Vec<u8>) -> Result<(), std::io::Error> {
        if let Some(ref mut writer) = self.writer {
            let entry = PacketEntry {
                timestamp: Utc::now().timestamp_millis(),
                direction,
                data,
                protocol_version: Some(self.protocol_version.clone()),
                packet_json: None,
                packet_number: None, // Binary logs don't have packet_number
            };

            // Serialize the packet entry using bincode
            // We write the length first so we can read entries back correctly
            let serialized = bincode::serialize(&entry)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            
            // Write length as u32 (little-endian) followed by data
            let len = serialized.len() as u32;
            writer.write_all(&len.to_le_bytes())?;
            writer.write_all(&serialized)?;
            writer.flush()?;
        }
        
        Ok(())
    }

    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    pub fn log_path(&self) -> &Path {
        &self.log_path
    }
}

impl Drop for PacketLogger {
    fn drop(&mut self) {
        if let Some(mut writer) = self.writer.take() {
            let _ = writer.flush();
            let _ = writer.into_inner()
                .map_err(|e| eprintln!("Error flushing log file: {}", e));
        }
    }
}
