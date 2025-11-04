-- Migration: Rename timestamp column to ts
-- Run this manually if you have an existing database with the old schema

-- Rename the column
ALTER TABLE packets RENAME COLUMN timestamp TO ts;

-- Rename the index
DROP INDEX IF EXISTS idx_packets_timestamp;
CREATE INDEX IF NOT EXISTS idx_packets_ts ON packets(ts);
