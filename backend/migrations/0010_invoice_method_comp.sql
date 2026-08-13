-- Comped invoices are issued with method 'comp', which the original CHECK
-- constraint doesn't allow. SQLite can't alter a constraint in place, so rebuild
-- the table per https://www.sqlite.org/lang_altertable.html#otheralter. That
-- procedure requires foreign key enforcement to be off, which is why migrations
-- get their own connection (see `db::create_pool`) — the pragma is a no-op
-- inside the transaction sqlx wraps each migration in. Dropping the old table
-- before renaming the new one into place is what leaves the REFERENCES clauses
-- in invoice_item, bolt11, intent, and checkout pointing at the right table.

CREATE TABLE new_invoice (
  id TEXT PRIMARY KEY,
  tenant_pubkey TEXT NOT NULL,
  method TEXT CHECK (method IS NULL OR method IN ('nwc','stripe','oob','comp')),
  amount INTEGER NOT NULL,
  period_start INTEGER NOT NULL,
  period_end INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  paid_at INTEGER,
  voided_at INTEGER,
  notified_at INTEGER,
  FOREIGN KEY (tenant_pubkey) REFERENCES tenant(pubkey)
);

INSERT INTO new_invoice
  (id, tenant_pubkey, method, amount, period_start, period_end, created_at, paid_at, voided_at, notified_at)
SELECT id, tenant_pubkey, method, amount, period_start, period_end, created_at, paid_at, voided_at, notified_at
FROM invoice;

DROP TABLE invoice;

ALTER TABLE new_invoice RENAME TO invoice;

CREATE INDEX idx_invoice_tenant_created ON invoice (tenant_pubkey, created_at);

-- Dunning scans a tenant's still-open invoices oldest-first to retry payment.
CREATE INDEX idx_invoice_open ON invoice (tenant_pubkey, created_at) WHERE paid_at IS NULL AND voided_at IS NULL;
