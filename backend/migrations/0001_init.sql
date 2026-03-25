CREATE TABLE IF NOT EXISTS activities (
  id TEXT PRIMARY KEY,
  created_at INTEGER NOT NULL,
  activity_type TEXT NOT NULL,
  identifier TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tenants (
  pubkey TEXT PRIMARY KEY,
  nwc_url TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  billing_anchor INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS relays (
  id TEXT PRIMARY KEY,
  tenant TEXT NOT NULL,
  schema TEXT NOT NULL,
  subdomain TEXT NOT NULL UNIQUE,
  plan TEXT NOT NULL,
  status TEXT NOT NULL,
  sync_error TEXT NOT NULL DEFAULT '',
  info_name TEXT NOT NULL DEFAULT '',
  info_icon TEXT NOT NULL DEFAULT '',
  info_description TEXT NOT NULL DEFAULT '',
  policy_public_join INTEGER NOT NULL DEFAULT 0,
  policy_strip_signatures INTEGER NOT NULL DEFAULT 0,
  groups_enabled INTEGER NOT NULL DEFAULT 1,
  management_enabled INTEGER NOT NULL DEFAULT 1,
  blossom_enabled INTEGER NOT NULL DEFAULT 0,
  livekit_enabled INTEGER NOT NULL DEFAULT 0,
  push_enabled INTEGER NOT NULL DEFAULT 1,
  FOREIGN KEY (tenant) REFERENCES tenants(pubkey)
);

CREATE TABLE IF NOT EXISTS invoices (
  id TEXT PRIMARY KEY,
  tenant TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  attempted_at INTEGER NOT NULL DEFAULT 0,
  error TEXT NOT NULL DEFAULT '',
  closed_at INTEGER NOT NULL DEFAULT 0,
  sent_at INTEGER NOT NULL DEFAULT 0,
  paid_at INTEGER NOT NULL DEFAULT 0,
  bolt11 TEXT NOT NULL,
  period_start INTEGER NOT NULL,
  period_end INTEGER NOT NULL,
  FOREIGN KEY (tenant) REFERENCES tenants(pubkey)
);

CREATE TABLE IF NOT EXISTS invoice_items (
  id TEXT PRIMARY KEY,
  invoice TEXT NOT NULL,
  relay TEXT NOT NULL,
  sats INTEGER NOT NULL,
  FOREIGN KEY (invoice) REFERENCES invoices(id),
  FOREIGN KEY (relay) REFERENCES relays(id)
);
