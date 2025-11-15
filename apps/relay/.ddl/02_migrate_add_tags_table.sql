-- Migration: Add tags and tag_maps tables
-- Run this manually if you have an existing database with the old schema

-- Create tags master table (unique tag strings)
CREATE TABLE IF NOT EXISTS tags (
    tag VARCHAR(255) PRIMARY KEY
);

-- Create tag maps table (maps tags to packets or sessions)
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

-- Create indexes for tag_maps table
CREATE INDEX IF NOT EXISTS idx_tag_maps_packet_id ON tag_maps(packet_id);
CREATE INDEX IF NOT EXISTS idx_tag_maps_session_id ON tag_maps(session_id);
CREATE INDEX IF NOT EXISTS idx_tag_maps_tag ON tag_maps(tag);

