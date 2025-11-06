# BedrockRelay Monorepo

A monorepo for Minecraft Bedrock packet relay and inspection tools. In operation, the relay sits between a bedrock server and client.
A client connects to the relay, which forwards packets to the server and logs them to a postgres database (see the included docker-compose.yaml).
The packets can be viewed and filtered from the database using the included CLI application, lazypacket.

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

### `@bedrockrelay/lazypacket`

Rust-based CLI tool for browsing and introspecting packets stored in PostgreSQL.

**Start lazypacket:**
```bash
pnpm start:lazypacket
# or
pnpm lazypacket
# or
pnpm --filter @bedrockrelay/lazypacket start
```

**Environment Variables:**

lazypacket loads environment variables from the `.env` file in the project root. The Rust binary uses the `dotenv` crate to automatically search for `.env` files in multiple locations:
1. Current working directory (`.env`)
2. Two levels up (`../../.env`) - project root when running from `apps/lazypacket/`
3. One level up (`../.env`) - project root when running from project root

Make sure your `.env` file contains the database connection settings:
- `DB_HOST` (default: localhost)
- `DB_PORT` (default: 5432)
- `DB_USER` (default: postgres)
- `DB_PASSWORD` (default: postgres)
- `DB_NAME` (default: postgres)

lazypacket will show helpful error messages if the database connection fails, including which connection parameters were used.

**Build:**
```bash
pnpm --filter @bedrockrelay/lazypacket build
# or
cd apps/lazypacket
cargo build --release --bin lazypacket
```

## Development

### Prerequisites

- Node.js >= 24.10.0
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

3. Configure environment variables (create `.env` file at project root):
```bash
# Database (used by both relay and lazypacket)
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

**Note:** Both the relay and lazypacket load the `.env` file from the project root:
- **Relay**: Explicitly loads `../../.env` relative to `apps/relay/relay.js`
- **lazypacket**: Uses `dotenv` crate to search multiple locations (current dir, `../../.env`, `../.env`)

### Available Scripts

- `pnpm build` - Build all apps
- `pnpm test` - Run tests for all apps
- `pnpm start` - Start all apps
- `pnpm start:relay` - Start only the relay server
- `pnpm start:lazypacket` or `pnpm lazypacket` - Start lazypacket
- `pnpm lazypacket:dev` - Start lazypacket in dev mode (debug build)
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
