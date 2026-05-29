CREATE TABLE IF NOT EXISTS tenant (
  pubkey TEXT PRIMARY KEY,
  nwc_url TEXT NOT NULL DEFAULT '',
  nwc_error TEXT,
  stripe_error TEXT,
  created_at INTEGER NOT NULL,
  billing_anchor INTEGER,
  stripe_customer_id TEXT NOT NULL,
  stripe_payment_method_id TEXT,
  renewed_at INTEGER,
  churned_at INTEGER
);

CREATE TABLE IF NOT EXISTS activity (
  id TEXT PRIMARY KEY,
  tenant_pubkey TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  activity_type TEXT NOT NULL,
  resource_type TEXT NOT NULL,
  resource_id TEXT NOT NULL,
  billed_at INTEGER,
  snapshot TEXT NOT NULL,
  FOREIGN KEY (tenant_pubkey) REFERENCES tenant(pubkey)
);


CREATE TABLE IF NOT EXISTS relay (
  id TEXT PRIMARY KEY,
  tenant_pubkey TEXT NOT NULL,
  subdomain TEXT NOT NULL UNIQUE,
  plan_id TEXT NOT NULL,
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
  FOREIGN KEY (tenant_pubkey) REFERENCES tenant(pubkey)
);

CREATE TABLE IF NOT EXISTS invoice (
  id TEXT PRIMARY KEY,
  tenant_pubkey TEXT NOT NULL,
  method TEXT CHECK (method IS NULL OR method IN ('nwc','stripe','oob')),
  amount INTEGER NOT NULL,
  period_start INTEGER NOT NULL,
  period_end INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  paid_at INTEGER,
  voided_at INTEGER,
  FOREIGN KEY (tenant_pubkey) REFERENCES tenant(pubkey)
);

CREATE TABLE IF NOT EXISTS invoice_item (
  id TEXT PRIMARY KEY,
  invoice_id TEXT,
  activity_id TEXT,
  tenant_pubkey TEXT NOT NULL,
  relay_id TEXT NOT NULL,
  plan_id TEXT NOT NULL,
  amount INTEGER NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  FOREIGN KEY (invoice_id) REFERENCES invoice(id),
  FOREIGN KEY (tenant_pubkey) REFERENCES tenant(pubkey)
);

CREATE TABLE IF NOT EXISTS bolt11 (
  id TEXT PRIMARY KEY,
  invoice_id TEXT NOT NULL,
  lnbc TEXT NOT NULL,
  msats INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  settled_at INTEGER,
  FOREIGN KEY (invoice_id) REFERENCES invoice(id)
);

CREATE TABLE IF NOT EXISTS intent (
  id TEXT PRIMARY KEY,
  invoice_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (invoice_id) REFERENCES invoice(id)
);

CREATE INDEX IF NOT EXISTS idx_activity_tenant_created ON activity (tenant_pubkey, created_at);

CREATE INDEX IF NOT EXISTS idx_activity_resource_created ON activity (resource_id, created_at);

CREATE INDEX IF NOT EXISTS idx_activity_unbilled ON activity (tenant_pubkey, created_at) WHERE billed_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_relay_tenant_pubkey ON relay (tenant_pubkey);

CREATE INDEX IF NOT EXISTS idx_invoice_tenant_created ON invoice (tenant_pubkey, created_at);

-- Dunning scans a tenant's still-open invoices oldest-first to retry payment.
CREATE INDEX IF NOT EXISTS idx_invoice_open ON invoice (tenant_pubkey, created_at) WHERE paid_at IS NULL AND voided_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_invoice_item_invoice ON invoice_item (invoice_id);

CREATE INDEX IF NOT EXISTS idx_invoice_item_outstanding ON invoice_item (tenant_pubkey) WHERE invoice_id IS NULL;

-- At most one line item per billable activity to ensure no double-billing.
CREATE UNIQUE INDEX IF NOT EXISTS idx_invoice_item_activity ON invoice_item (activity_id);

CREATE INDEX IF NOT EXISTS idx_bolt11_invoice_created ON bolt11 (invoice_id, created_at);
