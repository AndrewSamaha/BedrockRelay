# lazypacket

A terminal-based CLI tool for browsing and inspecting Minecraft Bedrock Edition packets stored in a PostgreSQL database. lazypacket provides an interactive TUI (Text User Interface) for exploring packet data captured by the BedrockRelay proxy.

## Features

- **Session Browser**: View all captured sessions with packet counts and duration
- **Packet Viewer**: Navigate through packets with detailed information
- **JSON & Hex Views**: Toggle between human-readable JSON and raw hex dump views
- **Direction Filtering**: Filter packets by direction (clientbound, serverbound, or all)
- **Protocol Parsing**: Automatic protocol version detection and packet identification
- **Timeline Visualization**: Visual timeline showing packet flow
- **Keyboard Navigation**: Vim-like keybindings for efficient navigation

## Installation

### Prerequisites

- Rust (latest stable version)
- PostgreSQL database with packets captured by BedrockRelay
- Environment variables configured (see below)

### Build

From the project root:
```bash
pnpm --filter @bedrockrelay/lazypacket build
# or
cd apps/lazypacket
cargo build --release --bin lazypacket
```

## Usage

### Start lazypacket

From the project root:
```bash
pnpm start:lazypacket
# or
pnpm lazypacket
# or
pnpm --filter @bedrockrelay/lazypacket start
```

From the lazypacket directory:
```bash
cargo run --release --bin lazypacket
# or for development (debug build)
cargo run --bin lazypacket
```

### Environment Variables

lazypacket loads environment variables from the `.env` file in the project root. The Rust binary uses the `dotenv` crate to automatically search for `.env` files in multiple locations:

1. Current working directory (`.env`)
2. Two levels up (`../../.env`) - project root when running from `apps/lazypacket/`
3. One level up (`../.env`) - project root when running from project root

Required database connection settings:
- `DB_HOST` (default: localhost)
- `DB_PORT` (default: 5432)
- `DB_USER` (default: postgres)
- `DB_PASSWORD` (default: postgres)
- `DB_NAME` (default: postgres)

If the database connection fails, lazypacket will show helpful error messages including which connection parameters were used.

## Keyboard Shortcuts

### Session List View

- `↑` / `↓` - Navigate sessions
- `Enter` - Open selected session
- `q` / `Esc` - Quit application

### Packet View

- `←` / `h` - Previous packet
- `→` / `l` - Next packet
- `↑` / `k` - Scroll packet details up
- `↓` / `j` - Scroll packet details down
- `PageUp` - Jump back 10 packets
- `PageDown` - Jump forward 10 packets
- `Home` - Jump to first packet
- `End` - Jump to last packet
- `x` / `X` - Toggle between JSON and hex view
- `f` / `F` - Enter filter mode
- `q` / `Esc` - Return to session list

### Filter Mode

- `c` - Filter to clientbound packets only
- `s` - Filter to serverbound packets only
- `a` - Show all packets (no filter)
- `Enter` - Apply filter
- `Esc` - Cancel filter and return to packet view
- `Backspace` - Clear filter input

## Architecture

### Source Structure

```
src/
├── lazypacket.rs    # Main application entry point and TUI
├── db.rs            # PostgreSQL database interface
├── protocol.rs      # Protocol parser for packet decoding
├── packet_logger.rs # Packet data structures
└── lib.rs           # Library module exports
```

### Key Components

- **Database Module** (`db.rs`): Handles PostgreSQL connections and queries for sessions and packets
- **Protocol Parser** (`protocol.rs`): Parses protocol YAML files and decodes packet structures
- **TUI Application** (`lazypacket.rs`): Ratatui-based terminal interface with session browsing and packet viewing

### Data Flow

1. Application connects to PostgreSQL database
2. Loads session list from `sessions` table
3. On session selection, loads packets from `packets` table
4. Displays packets with JSON or hex formatting
5. Optionally decodes packets using protocol parser for enhanced information

## Protocol Support

lazypacket includes protocol definitions for Minecraft Bedrock Edition version 1.21.111. The protocol parser can:
- Identify packets by name and ID
- Decode packet fields when protocol definitions are available
- Display protocol version in the UI

Protocol definitions are stored in `data/protocol/proto-1.21.111.yml`.

## Development

### Running in Development Mode

```bash
cargo run --bin lazypacket
```

This uses a debug build which is faster to compile but slower to run.

### Dependencies

Key dependencies:
- `ratatui` - Terminal UI framework
- `crossterm` - Cross-platform terminal manipulation
- `tokio-postgres` - Async PostgreSQL client
- `serde` / `serde_json` - Serialization
- `chrono` - Date/time handling
- `dotenv` - Environment variable loading

## License

ISC
