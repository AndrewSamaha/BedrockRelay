# Testing Protocol Decoding

## Test Results

### Build Status
- ? All binaries compile successfully
- ? Protocol module compiles with YAML parsing
- ? Viewer integrates protocol parser

### Protocol Parser Tests

#### Packet ID Extraction
- **Test**: `test_extract_packet_id`
- **Status**: ? Passing
- **Details**: Varint decoding correctly extracts packet IDs from byte arrays
- Single-byte varints (e.g., `0x01` ? 1)
- Multi-byte varints (e.g., `0x81 0x01` ? 129)

#### Protocol Loading
- **Test**: `test_protocol_loading`
- **Status**: ?? Needs investigation
- **Issue**: YAML parsing may have issues with certain syntax
- **Workaround**: Viewer gracefully handles parser load failures

### Manual Testing

#### Viewer Startup
1. ? Viewer starts successfully
2. ? Protocol parser attempts to load on startup
3. ? Graceful degradation if parser unavailable
4. ? No crashes or panics

#### Protocol File
- ? `data/protocol/proto-1.21.111.yml` exists
- ? File size: ~195KB (4580 lines)
- ? Contains packet definitions with IDs and field structures

## Testing Checklist

### Unit Tests
- [x] Packet ID extraction (varint decoding)
- [ ] Protocol definition loading
- [ ] Packet name lookup by ID
- [ ] Direction filtering (clientbound vs serverbound)

### Integration Tests
- [x] Viewer can load protocol parser
- [x] Viewer displays packet information when parser available
- [ ] Viewer shows raw data when parser unavailable
- [ ] Protocol version matching works correctly

### Manual Testing Steps

1. **Test with existing logs:**
   ```bash
   cargo run --bin viewer
   ```
   - Navigate to a log file
   - Check if protocol version is displayed
   - Verify packet names/IDs appear in JSON view

2. **Test with new logs:**
   ```bash
   cargo run  # Start proxy
   # Connect a Minecraft client
   # Stop proxy
   cargo run --bin viewer
   ```
   - Check new log appears in session list
   - Verify protocol version stored correctly
   - Check packet decoding works

3. **Test error handling:**
   - Remove `data/protocol/proto-1.21.111.yml`
   - Start viewer
   - Verify it still works (shows raw data)

## Known Issues

1. **YAML Parsing**: Some tests fail with YAML parsing errors
   - May be related to YAML tag syntax (`!id:`, `!bound:`)
   - Parser still works for basic packet definitions
   - Consider alternative parsing approach if needed

2. **RakNet Headers**: Packet ID extraction assumes raw packet data
   - May fail if RakNet headers present
   - Need to implement RakNet header parsing for production use

3. **Field Decoding**: Not yet implemented
   - Only packet identification works
   - Field values still show as raw byte arrays

## Next Testing Steps

1. Test with actual Minecraft Bedrock packets
2. Verify packet IDs match expected values
3. Test with different packet types
4. Validate protocol version matching
5. Test backward compatibility with old logs

## Test Data

To create test logs:
1. Start proxy: `cargo run`
2. Connect Minecraft Bedrock client
3. Perform some actions (login, move, etc.)
4. Stop proxy
5. View logs: `cargo run --bin viewer`

Expected results:
- Logs created in `logs/` directory
- Viewer shows session list
- Protocol version displayed
- Packet names/IDs visible in JSON view
