# Bedrock Rust Proxy

A high-performance Minecraft Bedrock proxy written in Rust that relays packets between clients and servers while logging all traffic.

## Features

- **Packet Relay**: Forwards packets between Minecraft Bedrock clients and servers
- **Binary Packet Logging**: Records all packets in binary format (compressed by default)
- **Session Management**: One log file per client session
- **Compressed Logs**: Packet logs are gzip-compressed to save disk space

## Building

```bash
cargo build --release
```

## Running

```bash
cargo run --release
```

The proxy is configured to:
- Listen on `0.0.0.0:19332` for client connections
- Forward packets to `192.168.1.100:19132` (upstream server)

To change these settings, edit the addresses in `src/main.rs`.

### Log Levels

By default, the proxy logs at `INFO` level, showing:
- Proxy startup and configuration
- New session creation
- Errors and warnings

To see more verbose output including packet forwarding details, set the `RUST_LOG` environment variable:

```bash
# Debug level - shows all packet operations
RUST_LOG=debug cargo run --release

# Trace level - most verbose (includes all tracing events)
RUST_LOG=trace cargo run --release

# Custom - set specific module levels
RUST_LOG=bedrock_rust_proxy=debug,tokio=warn cargo run --release
```

Available log levels (from most to least verbose):
- `trace`: Most detailed, includes all internal operations
- `debug`: Detailed packet operations and forwarding
- `info`: Important events (default)
- `warn`: Warnings only
- `error`: Errors only

## Packet Logging

All packets passing through the proxy are logged to the `logs/` directory. Each session gets its own log file:
- Format: `{session_id}.bin.gz` (compressed) or `{session_id}.bin` (uncompressed)
- Each packet entry contains:
  - Timestamp (milliseconds since epoch)
  - Direction (Clientbound or Serverbound)
  - Raw packet data

## Architecture

- **proxy.rs**: Main proxy server handling UDP connections
- **session.rs**: Session management and tracking
- **packet_logger.rs**: Binary packet logging with compression support

## Packet Log Viewer

The project includes a terminal-based viewer for browsing session logs:

```bash
cargo run --bin viewer
```

### Features

- **Session Selection**: Browse all available session logs in the `logs/` directory
- **Packet Navigation**: Navigate through packets with arrow keys or vim-style keys (h/j/k/l)
- **Timeline Visualization**: See a visual timeline of packet directions (? for clientbound in green, ? for serverbound in blue)
- **Packet Details**: View packet information in JSON format (default) or hex dump view (toggle with 'x')
- **Scrollable Content**: Packet Details panel is scrollable - use ?/? or k/j to scroll
- **Fast Navigation**: 
  - Arrow keys ?/? or h/l: navigate packets
  - Arrow keys ?/? or k/j: scroll packet details content
  - Page Up/Down: jump 10 packets backward/forward
  - Home/End: jump to first/last packet
  - x: toggle between JSON and hex view
  - q/Esc: return to session list or quit

### Controls

**Session List Mode:**
- `?` / `?`: Navigate sessions
- `Enter`: Select and view session
- `q` / `Esc`: Quit

**Packet View Mode:**
- `?` / `?` or `h` / `l`: Navigate packets
- `?` / `?` or `k` / `j`: Scroll packet details content
- `Page Up` / `Page Down`: Jump 10 packets backward/forward
- `Home`: Jump to first packet
- `End`: Jump to last packet
- `x`: Toggle between JSON and hex view
- `q` / `Esc`: Return to session list

## Future Features

- RakNet protocol parsing and packet identification
- JSON export of packet data
- Packet filtering and search
- Support for multiple concurrent clients with proper session tracking
- Configuration file support
- CLI arguments for proxy settings

## Dependencies

### Core Proxy
- `tokio`: Async runtime
- `bincode`: Binary serialization for packet logs
- `flate2`: Compression for log files
- `tracing`: Structured logging
- `chrono`: Timestamp generation
- `uuid`: Session ID generation

### Viewer Tool
- `ratatui`: Terminal UI framework for the packet viewer
- `crossterm`: Cross-platform terminal manipulation

## Notes

The current implementation uses a simplified approach for multi-client support. For production use with multiple concurrent clients, you'll need to implement proper session tracking by:
1. Parsing RakNet packet headers to identify clients
2. Creating separate upstream socket connections per client
3. Using packet inspection to route upstream packets to the correct client
