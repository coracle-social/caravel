CREATE TABLE IF NOT EXISTS invoice_manual_lightning_payment (
  invoice_id TEXT PRIMARY KEY,
  tenant_pubkey TEXT NOT NULL,
  bolt11 TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (tenant_pubkey) REFERENCES tenant(pubkey)
);

CREATE INDEX IF NOT EXISTS idx_invoice_manual_lightning_payment_tenant_pubkey
ON invoice_manual_lightning_payment (tenant_pubkey);
