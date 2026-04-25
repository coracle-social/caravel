CREATE TABLE IF NOT EXISTS invoice_nwc_payment (
  invoice_id TEXT PRIMARY KEY,
  tenant_pubkey TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('pending', 'paid')),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (tenant_pubkey) REFERENCES tenant(pubkey)
);

CREATE INDEX IF NOT EXISTS idx_invoice_nwc_payment_tenant_pubkey
ON invoice_nwc_payment (tenant_pubkey);