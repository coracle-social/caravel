# `pub struct Repo`

Repo is a wrapper around a sqlite pool which implements methods related to database access.

Members:

- `pool: sqlx::SqlitePool` - a sqlite connection pool

Notes:

- All public write methods should be run in a transaction so they're atomic
- All writes should be accompanied by an activity log entry of `(activity_type, identifier)`
- Database table names are singular: `activity`, `tenant`, `relay`, `invoice`, `invoice_item`

## `pub fn new() -> Self`

- Reads `DATABASE_URL` from environment
- Ensures that any directories referred to in `DATABASE_URL` exist
- Initializes its sqlx `pool`

## `pub fn migrate(&self) -> Result<()>`

- Runs migrations found in the `migrations` directory.

## `pub fn list_tenants(&self) -> Result<Vec<Tenant>>`

- Returns all tenants

## `pub fn get_tenant(&self, pubkey: &str) -> Result<Tenant>`

- Returns matching tenant

## `pub fn create_tenant(&self, tenant: &Tenant) -> Result<()>`

- Creates tenant, may throw sqlite uniqueness error on pubkey
- Logs activity as `(tenant_created, tenant_id)`

## `pub fn update_tenant_billing_anchor(&self, pubkey: &str, billing_anchor: i64) -> Result<()>`

- Updates the tenant's `billing_anchor`
- Logs activity as `(tenant_billing_anchor_updated, tenant_id)`

## `pub fn list_relays(&self, tenant_id: Option<&str>) -> Result<Vec<Relay>>`

- Returns all matching relays

## `pub fn get_relay(&self, id: &str) -> Result<Relay>`

- Returns matching relay

## `pub fn create_relay(&self, relay: &Relay) -> Result<()>`

- Creates relay, may throw sqlite uniqueness error on subdomain
- Sets relay status to `new`
- Logs activity as `(relay_created, relay_id)`

## `pub fn update_relay(&self, relay: &Relay) -> Result<()>`

- Updates relay, may throw sqlite uniqueness error on subdomain
- Logs activity as `(relay_updated, relay_id)`

## `pub fn deactivate_relay(&self, relay: &Relay) -> Result<()>`

- Sets relay status to `inactive`
- Logs activity as `(relay_deactivated, relay_id)`

## `pub fn activate_relay(&self, relay: &Relay) -> Result<()>`

- Sets relay status to `active`
- Logs activity as `(relay_activated, relay_id)`

## `pub fn fail_relay_sync(&self, relay: &Relay, sync_error: String) -> Result<()>`

- Sets relay status to `inactive`, sets `sync_error`
- Logs activity as `(relay_sync_failed, relay_id)`

## `pub fn create_invoice(&self, invoice: &Invoice, invoice_items: [&InvoiceItem]) -> Result<()>`

- Saves an `invoice` row and related `invoice_item` rows
- Logs activity as `(invoice_created, invoice_id)`

## `pub fn list_invoices(tenant_id: Option<&str>) -> Result<Vec<Invoice>>`

- Returns all matching invoices

## `pub fn mark_invoice_paid(&self, invoice_id: &str) -> Result<()>`

- Sets invoice status to `paid`
- Sets `paid_at` to now
- Clears `error` if set
- Logs activity as `(invoice_paid, invoice_id)`

## `pub fn mark_invoice_attempted(&self, invoice_id: &str, error: Option<&str>) -> Result<()>`

- Sets `attempted_at` to now
- Updates `error` if provided
- Leaves status as `pending`
- Logs activity as `(invoice_attempted, invoice_id)`

## `pub fn mark_invoice_sent(&self, invoice_id: &str) -> Result<()>`

- Sets `sent_at` to now
- Leaves status as `pending`
- Logs activity as `(invoice_sent, invoice_id)`

## `pub fn mark_invoice_closed(&self, invoice_id: &str) -> Result<()>`

- Sets invoice status to `closed`
- Sets `closed_at` to now
- Logs activity as `(invoice_closed, invoice_id)`

## `pub fn list_activity(&self, since: &i64, tenant: Option<&str>) -> Result<Vec<Activity>>`

- Returns all activity occuring after `since` matching `tenant`
