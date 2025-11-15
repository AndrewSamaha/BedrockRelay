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

-- Tags master table (unique tag strings)
CREATE TABLE IF NOT EXISTS tags (
    tag VARCHAR(255) PRIMARY KEY
);

-- Tag maps table (maps tags to packets or sessions)
CREATE TABLE IF NOT EXISTS tag_maps (
    id SERIAL PRIMARY KEY,
    tag VARCHAR(255) NOT NULL REFERENCES tags(tag) ON DELETE CASCADE,
    packet_id INTEGER REFERENCES packets(id) ON DELETE CASCADE,
    session_id INTEGER REFERENCES sessions(id) ON DELETE CASCADE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT tag_maps_target_check CHECK (
        (packet_id IS NOT NULL AND session_id IS NULL) OR
        (packet_id IS NULL AND session_id IS NOT NULL)
    )
);

-- Indexes for common query patterns
CREATE INDEX IF NOT EXISTS idx_packets_session_id ON packets(session_id);
CREATE INDEX IF NOT EXISTS idx_packets_ts ON packets(ts);
CREATE INDEX IF NOT EXISTS idx_packets_direction ON packets(direction);
CREATE INDEX IF NOT EXISTS idx_packets_server_version ON packets(server_version);
CREATE INDEX IF NOT EXISTS idx_packets_session_time_ms ON packets(session_time_ms);
CREATE INDEX IF NOT EXISTS idx_sessions_started_at ON sessions(started_at);
CREATE INDEX IF NOT EXISTS idx_tag_maps_packet_id ON tag_maps(packet_id);
CREATE INDEX IF NOT EXISTS idx_tag_maps_session_id ON tag_maps(session_id);
CREATE INDEX IF NOT EXISTS idx_tag_maps_tag ON tag_maps(tag);
