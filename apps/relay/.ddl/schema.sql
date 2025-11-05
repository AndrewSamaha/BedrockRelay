-- Sessions table
CREATE TABLE IF NOT EXISTS sessions (
    id SERIAL PRIMARY KEY,
    started_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    ended_at TIMESTAMP
);

-- Packets table
CREATE TABLE IF NOT EXISTS packets (
    id SERIAL PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    ts TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    session_time_ms BIGINT NOT NULL,
    packet_number BIGINT NOT NULL,
    server_version VARCHAR(50) NOT NULL,
    direction VARCHAR(20) NOT NULL,
    packet JSONB NOT NULL
);

-- Indexes for common query patterns
CREATE INDEX IF NOT EXISTS idx_packets_session_id ON packets(session_id);
CREATE INDEX IF NOT EXISTS idx_packets_ts ON packets(ts);
CREATE INDEX IF NOT EXISTS idx_packets_direction ON packets(direction);
CREATE INDEX IF NOT EXISTS idx_packets_server_version ON packets(server_version);
CREATE INDEX IF NOT EXISTS idx_packets_session_time_ms ON packets(session_time_ms);
CREATE INDEX IF NOT EXISTS idx_sessions_started_at ON sessions(started_at);
