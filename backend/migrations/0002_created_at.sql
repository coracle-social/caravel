ALTER TABLE relay ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_relay_created ON relay (created_at);
