// Protocol parsing module for Minecraft Bedrock Edition
// Loads protocol definitions from proto.yml and decodes packets

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::io::{Cursor, Read};
use anyhow::{Result, Context, anyhow};
use serde_yaml::Value as YamlValue;
use serde_json::Value as JsonValue;

// Target protocol version - we'll use the closest available to 1.21.113
pub const PROTOCOL_VERSION: &str = "1.21.111";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketInfo {
    pub id: u32,
    pub name: String,
    pub bound: PacketBound, // "client", "server", or "both"
    pub fields: HashMap<String, YamlValue>, // Field definitions
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PacketBound {
    Client,
    Server,
    Both,
}

impl PacketBound {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "client" => PacketBound::Client,
            "server" => PacketBound::Server,
            "both" => PacketBound::Both,
            _ => PacketBound::Both, // Default
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedPacket {
    pub packet_id: Option<u32>,
    pub packet_name: Option<String>,
    pub fields: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
enum ProtoType {
    // Primitives
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    Bool,
    // Varints and zigzag
    VarInt32,
    VarInt64,
    ZigZag32,
    ZigZag64,
    // Little-endian
    LI16,
    LI32,
    LI64,
    LU16,
    LU32,
    LU64,
    // Strings
    String(CountType), // varint-prefixed string
    LittleString,      // li32-prefixed string
    ShortString,       // li16-prefixed string
    LatinString,       // varint-prefixed latin1
    // Arrays/Buffers
    Buffer(CountType),
    Array(Box<ProtoType>, CountType), // Array of type with count type
    // Complex types
    UUID,
    Vec2F,
    Vec3F,
    // Nested
    Encapsulated(Box<ProtoType>),
    Container(String), // Reference to a container type name
    // Special
    Native(String),     // Native type (nbt, etc.) - just read as bytes
    RestBuffer,         // Read remaining bytes
}

#[derive(Debug, Clone, Copy)]
enum CountType {
    VarInt,
    ZigZag32,
    LI16,
    LI32,
    LI64,
    LU16,
    LU32,
    Fixed(usize),
}

struct BinaryDecoder<'a> {
    cursor: Cursor<&'a [u8]>,
    type_aliases: &'a HashMap<String, YamlValue>,
    containers: &'a HashMap<String, HashMap<String, YamlValue>>,
}

pub struct ProtocolParser {
    protocol_version: String,
    packet_id_to_info: HashMap<u32, PacketInfo>,
    // Separate maps for clientbound and serverbound packets
    clientbound_ids: Vec<u32>,
    serverbound_ids: Vec<u32>,
    // Type aliases and container definitions
    type_aliases: HashMap<String, YamlValue>,
    containers: HashMap<String, HashMap<String, YamlValue>>,
}

impl ProtocolParser {
    pub fn new(version: &str) -> Result<Self> {
        let protocol_file = format!("data/protocol/proto-{}.yml", version);
        let proto_path = Path::new(&protocol_file);
        
        Self::load_from_file(proto_path, version)
    }

    pub fn load_from_file(path: &Path, version: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read protocol file: {}", path.display()))?;
        
        // Parse YAML - use from_slice to handle single document
        let yaml: YamlValue = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse YAML from {}", path.display()))?;

        let mut packet_id_to_info = HashMap::new();
        let mut clientbound_ids = Vec::new();
        let mut serverbound_ids = Vec::new();
        let mut type_aliases = HashMap::new();
        let mut containers = HashMap::new();

        // Parse YAML structure
        if let YamlValue::Mapping(mapping) = yaml {
            for (key, value) in mapping {
                if let YamlValue::String(name) = key {
                    if name.starts_with("packet_") {
                        // Parse packet definition
                        if let YamlValue::Mapping(packet_def) = value {
                            let mut packet_id = None;
                            let mut bound = PacketBound::Both;
                            let mut fields = HashMap::new();

                            for (k, v) in packet_def {
                                if let YamlValue::String(key_str) = k {
                                    match key_str.as_str() {
                                        "!id" => {
                                            if let YamlValue::String(id_str) = &v {
                                                // Handle hex format like "0x01"
                                                packet_id = Some(
                                                    u32::from_str_radix(
                                                        id_str.trim_start_matches("0x"),
                                                        16,
                                                    )?,
                                                );
                                            } else if let Some(id_num) = v.as_u64() {
                                                packet_id = Some(id_num as u32);
                                            }
                                        }
                                        "!bound" => {
                                            if let YamlValue::String(bound_str) = &v {
                                                bound = PacketBound::from_str(&bound_str);
                                            }
                                        }
                                        _ => {
                                            // This is a field definition
                                            fields.insert(key_str.clone(), v.clone());
                                        }
                                    }
                                }
                            }

                            if let Some(id) = packet_id {
                                let packet_info = PacketInfo {
                                    id,
                                    name: name.clone(),
                                    bound,
                                    fields: fields.clone(),
                                };
                                
                                packet_id_to_info.insert(id, packet_info);

                                // Track which direction this packet can be used for
                                if bound == PacketBound::Client || bound == PacketBound::Both {
                                    clientbound_ids.push(id);
                                }
                                if bound == PacketBound::Server || bound == PacketBound::Both {
                                    serverbound_ids.push(id);
                                }
                            }
                        }
                    } else if !name.starts_with("!") {
                        // Could be a type alias or container definition
                        // Type aliases are simple mappings like "string: [...]"
                        // Containers are mappings with field definitions
                        match &value {
                            YamlValue::Sequence(_) | YamlValue::String(_) => {
                                // Likely a type alias
                                type_aliases.insert(name.clone(), value.clone());
                            }
                            YamlValue::Mapping(fields) => {
                                // Likely a container definition (has fields, not !id or !bound)
                                let mut container_fields = HashMap::new();
                                for (k, v) in fields {
                                    if let YamlValue::String(field_name) = k {
                                        if !field_name.starts_with("!") {
                                            container_fields.insert(field_name.clone(), v.clone());
                                        }
                                    }
                                }
                                if !container_fields.is_empty() {
                                    containers.insert(name.clone(), container_fields);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        Ok(Self {
            protocol_version: version.to_string(),
            packet_id_to_info,
            clientbound_ids,
            serverbound_ids,
            type_aliases,
            containers,
        })
    }

    pub fn version(&self) -> &str {
        &self.protocol_version
    }

    pub fn packet_count(&self) -> usize {
        self.packet_id_to_info.len()
    }

    /// Get packet info by ID
    pub fn get_packet_info(&self, id: u32) -> Option<&PacketInfo> {
        self.packet_id_to_info.get(&id)
    }

    /// Extract packet ID from raw bytes (after RakNet header)
    /// Bedrock protocol packets typically have:
    /// - RakNet header (varies in size)
    /// - Packet ID (varint, usually 1-2 bytes for most packets)
    pub fn extract_packet_id(&self, data: &[u8]) -> Option<u32> {
        if data.is_empty() {
            return None;
        }

        // Try to read a varint (most common packet ID format)
        // Varint encoding: each byte has a continuation bit in the MSB
        let mut result: u32 = 0;
        let mut shift = 0;
        
        for &byte in data.iter().take(5) {
            // Bedrock varints are typically little-endian
            result |= ((byte & 0x7F) as u32) << shift;
            shift += 7;
            
            if (byte & 0x80) == 0 {
                // No continuation bit
                return Some(result);
            }
        }
        
        // If we couldn't parse a varint, fall back to first byte
        if data.len() > 0 {
            Some(data[0] as u32)
        } else {
            None
        }
    }

    /// Decode a packet using protocol definitions
    pub fn decode_packet(
        &self,
        data: &[u8],
        _direction: crate::packet_logger::PacketDirection,
    ) -> DecodedPacket {
        let packet_id = self.extract_packet_id(data);
        
        let packet_info = packet_id.and_then(|id| self.get_packet_info(id));
        let packet_name = packet_info.map(|info| info.name.clone());
        
        let mut fields = HashMap::new();
        
        // If we have packet info, try to decode fields
        if let Some(info) = packet_info {
            // Skip past the packet ID (varint)
            let id_size = self.extract_packet_id(data)
                .and_then(|_| {
                    // Calculate varint size
                    let mut size = 0;
                    for &byte in data.iter().take(5) {
                        size += 1;
                        if (byte & 0x80) == 0 {
                            break;
                        }
                    }
                    Some(size)
                })
                .unwrap_or(1);
            
            let packet_data = &data[id_size..];
            
            let mut decoder = BinaryDecoder::new(
                packet_data,
                &self.type_aliases,
                &self.containers,
            );
            
            // Decode fields from packet definition
            match decoder.decode_fields(&info.fields) {
                Ok(decoded) => fields = decoded,
                Err(_e) => {
                    // On decode error, still return packet ID and name
                    // (could be due to missing data, wrong format, etc.)
                }
            }
        }
        
        DecodedPacket {
            packet_id,
            packet_name,
            fields,
        }
    }
}

impl<'a> BinaryDecoder<'a> {
    fn new(
        data: &'a [u8],
        type_aliases: &'a HashMap<String, YamlValue>,
        containers: &'a HashMap<String, HashMap<String, YamlValue>>,
    ) -> Self {
        Self {
            cursor: Cursor::new(data),
            type_aliases,
            containers,
        }
    }
    
    fn decode_fields(
        &mut self,
        field_defs: &HashMap<String, YamlValue>,
    ) -> Result<HashMap<String, JsonValue>> {
        let mut result = HashMap::new();
        
        // Sort fields by key for consistent processing
        let mut fields: Vec<_> = field_defs.iter().collect();
        fields.sort_by_key(|(k, _)| *k);
        
        for (field_name, field_def) in fields {
            // Skip conditional fields and metadata fields for now
            if field_name == "_" || field_name.starts_with("!") {
                continue;
            }
            
            // Parse the field type
            let proto_type = self.parse_type(field_def)?;
            
            // Decode the value
            match self.decode_value(&proto_type) {
                Ok(value) => {
                    result.insert(field_name.clone(), value);
                }
                Err(e) => {
                    // Continue with other fields on decode error
                    // Insert error placeholder
                    result.insert(
                        field_name.clone(),
                        JsonValue::String(format!("[decode_error: {}]", e)),
                    );
                    break; // Stop decoding on error to avoid cascading failures
                }
            }
        }
        
        Ok(result)
    }
    
    fn parse_type(&self, yaml_value: &YamlValue) -> Result<ProtoType> {
        match yaml_value {
            YamlValue::String(type_str) => {
                self.parse_type_string(type_str)
            }
            YamlValue::Sequence(seq) => {
                // Array type: ["buffer", {"countType": "varint"}]
                // Or: ["pstring", {"countType": "varint"}]
                if seq.len() >= 1 {
                    if let YamlValue::String(first) = &seq[0] {
                        match first.as_str() {
                            "buffer" | "Buffer" => {
                                let count_type = if seq.len() >= 2 {
                                    self.parse_count_type(&seq[1])?
                                } else {
                                    CountType::VarInt
                                };
                                Ok(ProtoType::Buffer(count_type))
                            }
                            "pstring" => {
                                let count_type = if seq.len() >= 2 {
                                    self.parse_count_type(&seq[1])?
                                } else {
                                    CountType::VarInt
                                };
                                Ok(ProtoType::String(count_type))
                            }
                            "encapsulated" => {
                                let inner_type = if seq.len() >= 2 {
                                    self.parse_type(&seq[1])?
                                } else {
                                    return Err(anyhow!("encapsulated requires inner type"));
                                };
                                Ok(ProtoType::Encapsulated(Box::new(inner_type)))
                            }
                            _ => Err(anyhow!("Unknown array type: {}", first)),
                        }
                    } else {
                        Err(anyhow!("Array type first element must be string"))
                    }
                } else {
                    Err(anyhow!("Array type must have at least one element"))
                }
            }
            _ => Err(anyhow!("Invalid type definition: {:?}", yaml_value)),
        }
    }
    
    fn parse_type_string(&self, type_str: &str) -> Result<ProtoType> {
        // Check type aliases first
        if let Some(alias_def) = self.type_aliases.get(type_str) {
            return self.parse_type(alias_def);
        }
        
        // Check for array syntax like "string[]varint" or "i32[]li16"
        if let Some(bracket_pos) = type_str.find("[]") {
            let element_type_str = &type_str[..bracket_pos];
            let count_type_str = &type_str[bracket_pos + 2..];
            
            let element_type = self.parse_type_string(element_type_str)?;
            let count_type = match count_type_str {
                "varint" => CountType::VarInt,
                "zigzag32" => CountType::ZigZag32,
                "li16" => CountType::LI16,
                "li32" => CountType::LI32,
                "li64" => CountType::LI64,
                "lu16" => CountType::LU16,
                "lu32" => CountType::LU32,
                _ => CountType::VarInt, // Default
            };
            
            return Ok(ProtoType::Array(Box::new(element_type), count_type));
        }
        
        // Check for container reference
        if self.containers.contains_key(type_str) {
            return Ok(ProtoType::Container(type_str.to_string()));
        }
        
        // Parse primitive types
        match type_str {
            "i8" => Ok(ProtoType::I8),
            "u8" => Ok(ProtoType::U8),
            "i16" => Ok(ProtoType::I16),
            "u16" => Ok(ProtoType::U16),
            "i32" => Ok(ProtoType::I32),
            "u32" => Ok(ProtoType::U32),
            "i64" => Ok(ProtoType::I64),
            "u64" => Ok(ProtoType::U64),
            "f32" => Ok(ProtoType::F32),
            "f64" => Ok(ProtoType::F64),
            "bool" => Ok(ProtoType::Bool),
            "varint" | "varint32" => Ok(ProtoType::VarInt32),
            "varint64" => Ok(ProtoType::VarInt64),
            "zigzag32" => Ok(ProtoType::ZigZag32),
            "zigzag64" => Ok(ProtoType::ZigZag64),
            "li16" => Ok(ProtoType::LI16),
            "li32" => Ok(ProtoType::LI32),
            "li64" => Ok(ProtoType::LI64),
            "lu16" => Ok(ProtoType::LU16),
            "lu32" => Ok(ProtoType::LU32),
            "lu64" => Ok(ProtoType::LU64),
            "string" => Ok(ProtoType::String(CountType::VarInt)),
            "LittleString" => Ok(ProtoType::LittleString),
            "ShortString" => Ok(ProtoType::ShortString),
            "LatinString" => Ok(ProtoType::LatinString),
            "uuid" => Ok(ProtoType::UUID),
            "vec2f" => Ok(ProtoType::Vec2F),
            "vec3f" => Ok(ProtoType::Vec3F),
            "restBuffer" => Ok(ProtoType::RestBuffer),
            s if s.starts_with("native:") => {
                Ok(ProtoType::Native(s.trim_start_matches("native:").to_string()))
            }
            _ => {
                // Try as container name
                if self.containers.contains_key(type_str) {
                    Ok(ProtoType::Container(type_str.to_string()))
                } else {
                    Err(anyhow!("Unknown type: {}", type_str))
                }
            }
        }
    }
    
    fn parse_count_type(&self, yaml_value: &YamlValue) -> Result<CountType> {
        if let YamlValue::Mapping(map) = yaml_value {
            if let Some(YamlValue::String(count_type)) = map.get(&YamlValue::String("countType".to_string())) {
                match count_type.as_str() {
                    "varint" => Ok(CountType::VarInt),
                    "zigzag32" => Ok(CountType::ZigZag32),
                    "li16" => Ok(CountType::LI16),
                    "li32" => Ok(CountType::LI32),
                    "li64" => Ok(CountType::LI64),
                    "lu16" => Ok(CountType::LU16),
                    "lu32" => Ok(CountType::LU32),
                    _ => Err(anyhow!("Unknown count type: {}", count_type)),
                }
            } else {
                Ok(CountType::VarInt) // Default
            }
        } else {
            Ok(CountType::VarInt) // Default
        }
    }
    
    fn decode_value(&mut self, proto_type: &ProtoType) -> Result<JsonValue> {
        match proto_type {
            ProtoType::I8 => {
                let mut buf = [0u8; 1];
                self.cursor.read_exact(&mut buf)?;
                Ok(JsonValue::Number((buf[0] as i8).into()))
            }
            ProtoType::U8 => {
                let mut buf = [0u8; 1];
                self.cursor.read_exact(&mut buf)?;
                Ok(JsonValue::Number(buf[0].into()))
            }
            ProtoType::I16 => {
                let mut buf = [0u8; 2];
                self.cursor.read_exact(&mut buf)?;
                let value = i16::from_le_bytes(buf);
                Ok(JsonValue::Number(value.into()))
            }
            ProtoType::U16 => {
                let mut buf = [0u8; 2];
                self.cursor.read_exact(&mut buf)?;
                let value = u16::from_le_bytes(buf);
                Ok(JsonValue::Number(value.into()))
            }
            ProtoType::I32 => {
                let mut buf = [0u8; 4];
                self.cursor.read_exact(&mut buf)?;
                let value = i32::from_le_bytes(buf);
                Ok(JsonValue::Number(value.into()))
            }
            ProtoType::U32 => {
                let mut buf = [0u8; 4];
                self.cursor.read_exact(&mut buf)?;
                let value = u32::from_le_bytes(buf);
                Ok(JsonValue::Number(value.into()))
            }
            ProtoType::I64 => {
                let mut buf = [0u8; 8];
                self.cursor.read_exact(&mut buf)?;
                let value = i64::from_le_bytes(buf);
                Ok(JsonValue::Number(value.into()))
            }
            ProtoType::U64 => {
                let mut buf = [0u8; 8];
                self.cursor.read_exact(&mut buf)?;
                let value = u64::from_le_bytes(buf);
                // JSON numbers are f64, so for large u64 we need to use string
                if value <= (i64::MAX as u64) {
                    Ok(JsonValue::Number(value.into()))
                } else {
                    Ok(JsonValue::String(value.to_string()))
                }
            }
            ProtoType::F32 => {
                let mut buf = [0u8; 4];
                self.cursor.read_exact(&mut buf)?;
                let value = f32::from_le_bytes(buf);
                Ok(JsonValue::Number(serde_json::Number::from_f64(value as f64)
                    .unwrap_or(serde_json::Number::from(0))))
            }
            ProtoType::F64 => {
                let mut buf = [0u8; 8];
                self.cursor.read_exact(&mut buf)?;
                let value = f64::from_le_bytes(buf);
                Ok(JsonValue::Number(serde_json::Number::from_f64(value)
                    .unwrap_or(serde_json::Number::from(0))))
            }
            ProtoType::Bool => {
                let mut buf = [0u8; 1];
                self.cursor.read_exact(&mut buf)?;
                Ok(JsonValue::Bool(buf[0] != 0))
            }
            ProtoType::VarInt32 => {
                let value = self.read_varint32()?;
                Ok(JsonValue::Number(value.into()))
            }
            ProtoType::VarInt64 => {
                let value = self.read_varint64()?;
                // JSON numbers are f64, so for large i64 we need to use string
                if value >= 0 && value <= (i64::MAX as u64) {
                    Ok(JsonValue::Number((value as i64).into()))
                } else {
                    Ok(JsonValue::String(value.to_string()))
                }
            }
            ProtoType::ZigZag32 => {
                let value = self.read_varint32()?;
                let decoded = ((value >> 1) as i32) ^ (-((value & 1) as i32));
                Ok(JsonValue::Number(decoded.into()))
            }
            ProtoType::ZigZag64 => {
                let value = self.read_varint64()?;
                let decoded = ((value >> 1) as i64) ^ (-((value & 1) as i64));
                // JSON numbers are f64, so for large i64 we need to use string
                Ok(JsonValue::String(decoded.to_string()))
            }
            ProtoType::LI16 => {
                let mut buf = [0u8; 2];
                self.cursor.read_exact(&mut buf)?;
                let value = i16::from_le_bytes(buf);
                Ok(JsonValue::Number(value.into()))
            }
            ProtoType::LI32 => {
                let mut buf = [0u8; 4];
                self.cursor.read_exact(&mut buf)?;
                let value = i32::from_le_bytes(buf);
                Ok(JsonValue::Number(value.into()))
            }
            ProtoType::LI64 => {
                let mut buf = [0u8; 8];
                self.cursor.read_exact(&mut buf)?;
                let value = i64::from_le_bytes(buf);
                Ok(JsonValue::String(value.to_string()))
            }
            ProtoType::LU16 => {
                let mut buf = [0u8; 2];
                self.cursor.read_exact(&mut buf)?;
                let value = u16::from_le_bytes(buf);
                Ok(JsonValue::Number(value.into()))
            }
            ProtoType::LU32 => {
                let mut buf = [0u8; 4];
                self.cursor.read_exact(&mut buf)?;
                let value = u32::from_le_bytes(buf);
                Ok(JsonValue::Number(value.into()))
            }
            ProtoType::LU64 => {
                let mut buf = [0u8; 8];
                self.cursor.read_exact(&mut buf)?;
                let value = u64::from_le_bytes(buf);
                Ok(JsonValue::String(value.to_string()))
            }
            ProtoType::String(count_type) => {
                let len = self.read_count(count_type)?;
                let mut buf = vec![0u8; len as usize];
                self.cursor.read_exact(&mut buf)?;
                let string = String::from_utf8_lossy(&buf).to_string();
                Ok(JsonValue::String(string))
            }
            ProtoType::LittleString => {
                let len = self.read_count(&CountType::LI32)?;
                let mut buf = vec![0u8; len as usize];
                self.cursor.read_exact(&mut buf)?;
                let string = String::from_utf8_lossy(&buf).to_string();
                Ok(JsonValue::String(string))
            }
            ProtoType::ShortString => {
                let len = self.read_count(&CountType::LI16)?;
                let mut buf = vec![0u8; len as usize];
                self.cursor.read_exact(&mut buf)?;
                let string = String::from_utf8_lossy(&buf).to_string();
                Ok(JsonValue::String(string))
            }
            ProtoType::LatinString => {
                let len = self.read_count(&CountType::VarInt)?;
                let mut buf = vec![0u8; len as usize];
                self.cursor.read_exact(&mut buf)?;
                // Latin1 encoding: each byte is a character
                let string: String = buf.iter().map(|&b| b as char).collect();
                Ok(JsonValue::String(string))
            }
            ProtoType::UUID => {
                let mut buf = [0u8; 16];
                self.cursor.read_exact(&mut buf)?;
                // UUID format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
                let mut uuid_str = String::with_capacity(36);
                for (i, &byte) in buf.iter().enumerate() {
                    if i == 4 || i == 6 || i == 8 || i == 10 {
                        uuid_str.push('-');
                    }
                    uuid_str.push_str(&format!("{:02x}", byte));
                }
                Ok(JsonValue::String(uuid_str))
            }
            ProtoType::Vec2F => {
                let x = match self.decode_value(&ProtoType::F32)? {
                    JsonValue::Number(n) => n.as_f64().unwrap_or(0.0),
                    _ => 0.0,
                };
                let y = match self.decode_value(&ProtoType::F32)? {
                    JsonValue::Number(n) => n.as_f64().unwrap_or(0.0),
                    _ => 0.0,
                };
                Ok(JsonValue::Object({
                    let mut map = serde_json::Map::new();
                    map.insert("x".to_string(), JsonValue::Number(serde_json::Number::from_f64(x).unwrap()));
                    map.insert("y".to_string(), JsonValue::Number(serde_json::Number::from_f64(y).unwrap()));
                    map
                }))
            }
            ProtoType::Vec3F => {
                let x = match self.decode_value(&ProtoType::F32)? {
                    JsonValue::Number(n) => n.as_f64().unwrap_or(0.0),
                    _ => 0.0,
                };
                let y = match self.decode_value(&ProtoType::F32)? {
                    JsonValue::Number(n) => n.as_f64().unwrap_or(0.0),
                    _ => 0.0,
                };
                let z = match self.decode_value(&ProtoType::F32)? {
                    JsonValue::Number(n) => n.as_f64().unwrap_or(0.0),
                    _ => 0.0,
                };
                Ok(JsonValue::Object({
                    let mut map = serde_json::Map::new();
                    map.insert("x".to_string(), JsonValue::Number(serde_json::Number::from_f64(x).unwrap()));
                    map.insert("y".to_string(), JsonValue::Number(serde_json::Number::from_f64(y).unwrap()));
                    map.insert("z".to_string(), JsonValue::Number(serde_json::Number::from_f64(z).unwrap()));
                    map
                }))
            }
            ProtoType::Buffer(count_type) => {
                let len = self.read_count(count_type)?;
                let mut buf = vec![0u8; len as usize];
                self.cursor.read_exact(&mut buf)?;
                // Return as hex string for readability
                let hex = buf.iter().map(|b| format!("{:02x}", b)).collect::<String>();
                Ok(JsonValue::String(format!("0x{}", hex)))
            }
            ProtoType::Array(element_type, count_type) => {
                let count = self.read_count(count_type)?;
                let mut array = Vec::new();
                for _ in 0..count {
                    array.push(self.decode_value(element_type)?);
                }
                Ok(JsonValue::Array(array))
            }
            ProtoType::Encapsulated(inner_type) => {
                // Read length prefix (varint)
                let len = self.read_varint32()?;
                // Save current position
                let start_pos = self.cursor.position();
                // Decode inner type
                let value = self.decode_value(inner_type)?;
                // Verify we read the expected amount
                let read = self.cursor.position() - start_pos;
                if read != len as u64 {
                    eprintln!("Warning: Encapsulated length mismatch: expected {}, read {}", len, read);
                }
                Ok(value)
            }
            ProtoType::Container(name) => {
                if let Some(container_fields) = self.containers.get(name) {
                    let fields_map = self.decode_fields(container_fields)?;
                    Ok(JsonValue::Object(fields_map.into_iter().collect()))
                } else {
                    Err(anyhow!("Container '{}' not found", name))
                }
            }
            ProtoType::Native(_) => {
                // For native types, just read as hex string
                // In a full implementation, we'd parse NBT, etc.
                let remaining = self.cursor.get_ref().len() - self.cursor.position() as usize;
                let mut buf = vec![0u8; remaining.min(1024)]; // Limit to 1KB
                self.cursor.read_exact(&mut buf)?;
                let hex = buf.iter().map(|b| format!("{:02x}", b)).collect::<String>();
                Ok(JsonValue::String(format!("[native: 0x{}]", hex)))
            }
            ProtoType::RestBuffer => {
                let remaining = self.cursor.get_ref().len() - self.cursor.position() as usize;
                let mut buf = vec![0u8; remaining];
                self.cursor.read_exact(&mut buf)?;
                let hex = buf.iter().map(|b| format!("{:02x}", b)).collect::<String>();
                Ok(JsonValue::String(format!("0x{}", hex)))
            }
        }
    }
    
    fn read_varint32(&mut self) -> Result<u32> {
        let mut result: u32 = 0;
        let mut shift = 0;
        
        for _ in 0..5 {
            let mut buf = [0u8; 1];
            self.cursor.read_exact(&mut buf)?;
            let byte = buf[0];
            
            result |= ((byte & 0x7F) as u32) << shift;
            shift += 7;
            
            if (byte & 0x80) == 0 {
                return Ok(result);
            }
        }
        
        Err(anyhow!("Varint32 overflow"))
    }
    
    fn read_varint64(&mut self) -> Result<u64> {
        let mut result: u64 = 0;
        let mut shift = 0;
        
        for _ in 0..10 {
            let mut buf = [0u8; 1];
            self.cursor.read_exact(&mut buf)?;
            let byte = buf[0];
            
            result |= ((byte & 0x7F) as u64) << shift;
            shift += 7;
            
            if (byte & 0x80) == 0 {
                return Ok(result);
            }
        }
        
        Err(anyhow!("Varint64 overflow"))
    }
    
    fn read_count(&mut self, count_type: &CountType) -> Result<u32> {
        match count_type {
            CountType::VarInt => self.read_varint32(),
            CountType::ZigZag32 => {
                let value = self.read_varint32()?;
                Ok(((value >> 1) as i32 ^ (-((value & 1) as i32))) as u32)
            }
            CountType::LI16 => {
                let mut buf = [0u8; 2];
                self.cursor.read_exact(&mut buf)?;
                Ok(i16::from_le_bytes(buf) as u32)
            }
            CountType::LI32 => {
                let mut buf = [0u8; 4];
                self.cursor.read_exact(&mut buf)?;
                Ok(i32::from_le_bytes(buf) as u32)
            }
            CountType::LI64 => {
                let mut buf = [0u8; 8];
                self.cursor.read_exact(&mut buf)?;
                Ok(i64::from_le_bytes(buf) as u32)
            }
            CountType::LU16 => {
                let mut buf = [0u8; 2];
                self.cursor.read_exact(&mut buf)?;
                Ok(u16::from_le_bytes(buf) as u32)
            }
            CountType::LU32 => {
                let mut buf = [0u8; 4];
                self.cursor.read_exact(&mut buf)?;
                Ok(u32::from_le_bytes(buf))
            }
            CountType::Fixed(n) => Ok(*n as u32),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_loading() {
        let parser = ProtocolParser::new("1.21.111");
        assert!(parser.is_ok());
        if let Ok(p) = parser {
            assert!(p.packet_count() > 0);
        }
    }

    #[test]
    fn test_extract_packet_id() {
        let parser = ProtocolParser::new("1.21.111").unwrap();
        
        // Test varint extraction: 0x01 should decode to 1
        let data = vec![0x01];
        assert_eq!(parser.extract_packet_id(&data), Some(1));
        
        // Test larger varint: 0x81 0x01 decodes to 129
        let data = vec![0x81, 0x01];
        assert_eq!(parser.extract_packet_id(&data), Some(129));
    }
}
