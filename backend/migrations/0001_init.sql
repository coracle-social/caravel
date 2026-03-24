CREATE TABLE IF NOT EXISTS tenants (
  pubkey TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  nwc_url TEXT NOT NULL DEFAULT "",
  created_at INTEGER NOT NULL DEFAULT (UNIXEPOCH()),
  billing_anchor_at INTEGER NOT NULL DEFAULT (UNIXEPOCH()),
  stripe_customer_id TEXT NOT NULL DEFAULT '',
  stripe_subscription_id TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS relays (
  id TEXT PRIMARY KEY,
  tenant TEXT NOT NULL,
  name TEXT NOT NULL,
  subdomain TEXT NOT NULL UNIQUE,
  description TEXT NOT NULL,
  plan TEXT NOT NULL,
  status TEXT NOT NULL,
  icon TEXT NOT NULL DEFAULT "",
  config TEXT,
  FOREIGN KEY (tenant) REFERENCES tenants(pubkey)
);

CREATE TABLE IF NOT EXISTS invoices (
  id TEXT PRIMARY KEY,
  tenant TEXT NOT NULL,
  amount INTEGER NOT NULL,
  status TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  bolt11 TEXT NOT NULL,
  period_start INTEGER NOT NULL,
  period_end INTEGER NOT NULL,
  FOREIGN KEY (tenant) REFERENCES tenants(pubkey)
);

CREATE UNIQUE INDEX IF NOT EXISTS invoices_tenant_period_unique
  ON invoices (tenant, period_start, period_end);

CREATE TABLE IF NOT EXISTS invoice_items (
  id TEXT PRIMARY KEY,
  invoice TEXT NOT NULL,
  relay TEXT NOT NULL,
  amount INTEGER NOT NULL,
  period_start INTEGER NOT NULL,
  period_end INTEGER NOT NULL,
  FOREIGN KEY (invoice) REFERENCES invoices(id),
  FOREIGN KEY (relay) REFERENCES relays(id)
);

CREATE TABLE IF NOT EXISTS plans (
  id TEXT PRIMARY KEY,
  sats_per_month INTEGER NOT NULL
);

INSERT OR IGNORE INTO plans (id, sats_per_month) VALUES
  ('free',   0),
  ('basic',  10000),
  ('growth', 50000);

CREATE TABLE IF NOT EXISTS relay_lifecycle_events (
  id TEXT PRIMARY KEY,
  relay TEXT NOT NULL,
  tenant TEXT NOT NULL,
  event_type TEXT NOT NULL,
  plan TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (relay) REFERENCES relays(id),
  FOREIGN KEY (tenant) REFERENCES tenants(pubkey)
);

CREATE INDEX IF NOT EXISTS relay_lifecycle_events_relay_idx
  ON relay_lifecycle_events (relay, created_at);

CREATE INDEX IF NOT EXISTS relay_lifecycle_events_tenant_idx
  ON relay_lifecycle_events (tenant, created_at);

CREATE TABLE IF NOT EXISTS invoice_attempts (
  id TEXT PRIMARY KEY,
  invoice TEXT NOT NULL,
  run_id TEXT NOT NULL,
  method TEXT NOT NULL,
  outcome TEXT NOT NULL,
  error TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  FOREIGN KEY (invoice) REFERENCES invoices(id)
);

CREATE INDEX IF NOT EXISTS invoice_attempts_invoice_idx
  ON invoice_attempts (invoice, created_at);

CREATE INDEX IF NOT EXISTS invoice_attempts_run_id_idx
  ON invoice_attempts (run_id);
