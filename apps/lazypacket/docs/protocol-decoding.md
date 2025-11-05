# Protocol Decoding Feature

## Overview

The protocol decoding feature allows the packet viewer to display human-readable packet information instead of raw byte arrays. Packets are identified by their names and IDs, and eventually will show decoded field values.

## Target Protocol Version

- **Target**: Minecraft Bedrock Edition 1.21.113
- **Actual Version Used**: 1.21.111 (closest available in minecraft-data)

## Implementation Status

### ? Completed

1. **Protocol Definitions Loading**
   - Protocol definitions extracted from `minecraft-data` npm package
   - `proto.yml` file copied to `data/protocol/proto-1.21.111.yml`
   - `ProtocolParser` struct created to load and parse protocol definitions
   - Extracts packet IDs, names, direction (clientbound/serverbound), and field definitions from YAML

2. **Packet ID Extraction**
   - Implemented varint decoding for extracting packet IDs from raw bytes
   - Handles both single-byte and multi-byte varints
   - Current implementation reads directly from packet data (RakNet header parsing not yet implemented)

3. **Viewer Integration**
   - Protocol parser loaded on viewer startup
   - Gracefully handles parser load failures (shows raw data if parser unavailable)
   - Decoded packets display:
     - `packet_name`: Human-readable packet name (e.g., "packet_login")
     - `packet_id`: Packet ID in hex format (e.g., "0x01")
   - Protocol version displayed in viewer header

4. **Protocol Version Storage**
   - Protocol version stored in each `PacketEntry` when logging
   - Protocol version displayed in viewer header
   - Default version: "1.21.111"

### ?? In Progress

1. **YAML Parsing**
   - Basic YAML parsing implemented using `serde_yaml`
   - YAML tags (`!id:`, `!bound:`, etc.) handled in parsing logic
   - Some edge cases may need refinement

### ? Pending

1. **Field Decoding**
   - Implement full protodef-like type system parser
   - Decode packet fields based on definitions in `proto.yml`
   - Support for:
     - Primitive types (i32, u64, bool, etc.)
     - Strings (varint-prefixed, little-endian, etc.)
     - Arrays (with various count types)
     - Nested containers
     - Complex types (NBT, UUIDs, etc.)
   - Display decoded fields in viewer instead of raw byte array

2. **RakNet Header Parsing**
   - Properly parse RakNet packet headers
   - Extract packet ID after header
   - Handle RakNet packet flags and metadata

3. **Protocol Version Matching**
   - Match log protocol version with appropriate parser
   - Support multiple protocol versions
   - Graceful fallback if exact version not available

## File Structure

```
data/
  protocol/
    proto-1.21.111.yml    # Protocol definitions from minecraft-data

src/
  protocol.rs              # Protocol parsing module
  packet_logger.rs         # Updated to store protocol version
  viewer.rs                # Updated to use protocol parser
```

## Data Flow

1. **Packet Logging**
   - Proxy captures raw packet bytes
   - Protocol version (1.21.111) stored with each packet
   - Raw bytes stored in `PacketEntry`

2. **Packet Viewing**
   - Viewer loads protocol parser for version 1.21.111
   - For each packet:
     - Extract packet ID using varint decoder
     - Look up packet name from protocol definitions
     - Display packet name, ID, and raw data
   - Future: Decode fields and show human-readable values

## Usage

### Protocol Parser

```rust
// Load parser for specific version
let parser = ProtocolParser::new("1.21.111")?;

// Extract packet ID from raw bytes
let packet_id = parser.extract_packet_id(&packet_data);

// Decode packet
let decoded = parser.decode_packet(&packet_data, direction);
```

### Viewer

The viewer automatically loads the protocol parser and displays decoded information:
- Packet name appears in JSON view
- Packet ID appears in hex format
- Protocol version shown in header

## Known Issues

1. **YAML Parsing**: Some YAML syntax may not parse correctly (test failures observed)
2. **RakNet Headers**: Packet ID extraction currently assumes raw packet data, doesn't parse RakNet headers
3. **Field Decoding**: Not yet implemented - only packet identification works

## Future Enhancements

1. Full field decoding with all type support
2. Multiple protocol version support
3. Packet filtering/search by name or ID
4. Protocol-aware packet validation
5. Export decoded packets to structured formats (JSON, CSV, etc.)

## References

- Protocol definitions from: `bedrock-protocol` npm package
- Protocol format: ProtoDef/YAML-based definitions
- Documentation: See `proto.yml` comments and minecraft-data repository
