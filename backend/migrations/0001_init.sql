CREATE TABLE IF NOT EXISTS activity (
  id TEXT PRIMARY KEY,
  tenant TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  activity_type TEXT NOT NULL,
  resource_type TEXT NOT NULL,
  resource_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tenant (
  pubkey TEXT PRIMARY KEY,
  nwc_url TEXT NOT NULL DEFAULT '',
  nwc_error TEXT,
  created_at INTEGER NOT NULL,
  stripe_customer_id TEXT NOT NULL,
  stripe_subscription_id TEXT,
  past_due_at INTEGER
);

CREATE TABLE IF NOT EXISTS relay (
  id TEXT PRIMARY KEY,
  tenant TEXT NOT NULL,
  schema TEXT NOT NULL,
  subdomain TEXT NOT NULL UNIQUE,
  plan TEXT NOT NULL,
  status TEXT NOT NULL,
  synced INTEGER NOT NULL DEFAULT 0,
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
  FOREIGN KEY (tenant) REFERENCES tenant(pubkey)
);

CREATE TABLE IF NOT EXISTS lightning_invoice (
  stripe_invoice_id TEXT PRIMARY KEY,
  tenant_pubkey TEXT NOT NULL,
  bolt11 TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending', 'paid')),
  paid_method TEXT CHECK (paid_method IN ('nwc', 'manual')),
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (tenant_pubkey) REFERENCES tenant(pubkey)
);

CREATE INDEX IF NOT EXISTS idx_tenant_stripe_customer_id
  ON tenant (stripe_customer_id);

CREATE INDEX IF NOT EXISTS idx_relay_tenant_id
  ON relay (tenant, id);

CREATE INDEX IF NOT EXISTS idx_relay_tenant_status_plan
  ON relay (tenant, status, plan);

CREATE INDEX IF NOT EXISTS idx_activity_resource_type_resource_id_created_at_id
  ON activity (resource_type, resource_id, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_lightning_invoice_tenant_pubkey
  ON lightning_invoice (tenant_pubkey);
