-- Null for tenants that predate metering: no period has had its overage settled
-- yet, so the first pass bills the period that closes after this deploy.
ALTER TABLE tenant ADD COLUMN overage_billed_at INTEGER;

CREATE TABLE IF NOT EXISTS usage_sample (
  id TEXT PRIMARY KEY,
  relay_id TEXT NOT NULL,
  tenant_pubkey TEXT NOT NULL,
  storage_bytes INTEGER NOT NULL,
  participant_seconds INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (relay_id) REFERENCES relay(id),
  FOREIGN KEY (tenant_pubkey) REFERENCES tenant(pubkey)
);

-- Closing a period rolls up every sample one tenant took inside it.
CREATE INDEX idx_usage_sample_tenant_created ON usage_sample (tenant_pubkey, created_at);

-- That rollup also reaches back for each relay's last sample before the period,
-- to use as the baseline the period's participant clock is measured against.
CREATE INDEX idx_usage_sample_relay_created ON usage_sample (relay_id, created_at);
