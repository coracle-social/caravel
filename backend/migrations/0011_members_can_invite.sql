-- Defaults to on: before this column, every relay was synced with an invite-capable
-- member role, so existing relays keep that behavior.
ALTER TABLE relay ADD COLUMN members_can_invite INTEGER NOT NULL DEFAULT 1;
