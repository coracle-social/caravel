CREATE TABLE IF NOT EXISTS tenants (
  pubkey TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  tenant_nwc_url TEXT NOT NULL DEFAULT ""
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
  created_at TEXT NOT NULL,
  invoice TEXT NOT NULL,
  FOREIGN KEY (tenant) REFERENCES tenants(pubkey)
);

CREATE TABLE IF NOT EXISTS invoice_items (
  id TEXT PRIMARY KEY,
  invoice TEXT NOT NULL,
  relay TEXT NOT NULL,
  amount INTEGER NOT NULL,
  period_start TEXT NOT NULL,
  period_end TEXT NOT NULL,
  FOREIGN KEY (invoice) REFERENCES invoices(id),
  FOREIGN KEY (relay) REFERENCES relays(id)
);
