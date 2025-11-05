# BedrockRelay Monorepo

A monorepo for Minecraft Bedrock packet relay and inspection tools.

## Structure

```
BedrockRelay/
??? apps/
?   ??? relay/          # JavaScript relay server (captures packets to PostgreSQL)
?   ??? lazypacket/     # Rust CLI tool for browsing and introspecting packets
??? docker-compose.yaml # PostgreSQL database setup
??? turbo.json          # Turborepo configuration
??? package.json        # Root package configuration
```

## Apps

### `@bedrockrelay/relay`

Minecraft Bedrock relay server that:
- Relays packets between clients and servers
- Captures and saves clientbound and serverbound packets to PostgreSQL as JSONB
- Manages session tracking and graceful shutdown

**Start the relay:**
```bash
pnpm start:relay
# or
pnpm --filter @bedrockrelay/relay start
```

### `lazypacket`

Rust-based CLI tool for browsing and introspecting packets stored in PostgreSQL.

**Build:**
```bash
cd apps/lazypacket
cargo build --release
```

**Run:**
```bash
cd apps/lazypacket
cargo run --release
```

## Development

### Prerequisites

- Node.js >= 18.0.0
- pnpm >= 8.0.0
- Rust (for lazypacket)
- Docker & Docker Compose (for PostgreSQL)

### Setup

1. Install dependencies:
```bash
pnpm install
```

2. Start PostgreSQL database:
```bash
docker compose up -d
```

3. Configure environment variables (create `.env` file):
```bash
# Database
DB_HOST=localhost
DB_PORT=5432
DB_USER=postgres
DB_PASSWORD=postgres
DB_NAME=postgres

# Relay Configuration
BEDROCK_VERSION=1.21.0
PROXY_LISTENING_ADDRESS=0.0.0.0
PROXY_LISTENING_PORT=19131
PROXY_DESTINATION_ADDRESS=192.168.1.100
PROXY_DESTINATION_PORT=19132
```

### Available Scripts

- `pnpm build` - Build all apps
- `pnpm test` - Run tests for all apps
- `pnpm start` - Start all apps
- `pnpm start:relay` - Start only the relay server
- `pnpm db:wipe` - Wipe database volumes

### Turborepo

This monorepo uses [Turborepo](https://turbo.build/repo) for:
- Fast builds with intelligent caching
- Parallel task execution
- Dependency-aware task scheduling

## Database

The relay stores packets in PostgreSQL:
- **sessions**: Tracks client sessions with start/end times
- **packets**: Stores all packets with session reference, timestamps, and JSONB packet data

See `apps/relay/.ddl/schema.sql` for the full schema.

## License

ISC
