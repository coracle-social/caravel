# `pub struct Repo`

Repo is a wrapper around a sqlite pool which implements methods related to database access.

Members:

- `pool: sqlx::SqlitePool` - a sqlite connection pool

Notes:

- All public write methods should be run in a transaction so they're atomic
- All writes should be accompanied by an activity log entry of `(tenant, activity_type, resource_type, resource_id)`
- Database table names are singular: `activity`, `tenant`, `relay`, `invoice`, `invoice_item`

## `pub fn new() -> Self`

- Reads `DATABASE_URL` from environment
- Ensures that any directories referred to in `DATABASE_URL` exist
- Initializes its sqlx `pool`
- Runs migrations found in the `migrations` directory.

## `fn insert_activity(activity_type, resource_type, resource_id) -> Result<()>`

- Private helper that inserts one row into `activity`
- Infers `tenant` from `resource_type` and `resource_id`
- Used by write methods to avoid repeating audit-log SQL

## `pub fn list_tenants(&self) -> Result<Vec<Tenant>>`

- Returns all tenants

## `pub fn get_tenant(&self, pubkey: &str) -> Result<Tenant>`

- Returns matching tenant

## `pub fn create_tenant(&self, tenant: &Tenant) -> Result<()>`

- Creates tenant, may throw sqlite uniqueness error on pubkey
- Logs activity as `(create_tenant, tenant_id)`

## `pub fn update_tenant_billing_anchor(&self, pubkey: &str, billing_anchor: i64) -> Result<()>`

- Updates the tenant's `billing_anchor`
- Logs activity as `(update_tenant_billing_anchor, tenant_id)`

## `pub fn update_tenant_nwc_url(&self, pubkey: &str, nwc_url: &str) -> Result<()>`

- Updates tenant `nwc_url`
- Logs activity as `(update_tenant_nwc_url, tenant_id)`

## `pub fn list_plans() -> Vec<Plan>`

- Returns the hardcoded relay plans used by the system (`free`, `basic`, `growth`)
- This is the source of truth for plan metadata exposed via API

## `pub fn list_relays(&self) -> Result<Vec<Relay>>`

- Returns all relays

## `pub fn list_relays_for_tenant(&self, tenant_id: &str) -> Result<Vec<Relay>>`

- Returns all relays belonging to the given tenant

## `pub fn get_relay(&self, id: &str) -> Result<Relay>`

- Returns matching relay

## `pub fn create_relay(&self, relay: &Relay) -> Result<()>`

- Creates relay, may throw sqlite uniqueness error on subdomain
- Sets relay status to `new`
- Logs activity as `(create_relay, relay_id)`

## `pub fn update_relay(&self, relay: &Relay) -> Result<()>`

- Updates relay, may throw sqlite uniqueness error on subdomain
- Logs activity as `(update_relay, relay_id)`

## `pub fn deactivate_relay(&self, relay: &Relay) -> Result<()>`

- Sets relay status to `inactive`
- Logs activity as `(deactivate_relay, relay_id)`

## `pub fn activate_relay(&self, relay: &Relay) -> Result<()>`

- Sets relay status to `active`
- Logs activity as `(activate_relay, relay_id)`

## `pub fn fail_relay_sync(&self, relay: &Relay, sync_error: &str) -> Result<()>`

- Sets relay status to `inactive`, sets `sync_error`
- Logs activity as `(fail_relay_sync, relay_id)`

## `pub fn mark_relay_synced(&self, relay_id: &str) -> Result<()>`

- Sets `synced = 1`, `status = 'active'`, clears `sync_error`
- No activity log (called by infra after successful sync)

## `pub fn max_activity_at(&self) -> Result<i64>`

- Returns the maximum `created_at` value from the activity table, or 0 if empty
- Used by infra to initialize the since guard on startup

## `pub fn create_invoice(&self, invoice: &Invoice, invoice_items: [&InvoiceItem]) -> Result<()>`

- Saves an `invoice` row and related `invoice_item` rows
- Logs activity as `(create_invoice, invoice_id)`

## `pub fn list_invoices() -> Result<Vec<Invoice>>`

- Returns all invoices

## `pub fn list_invoices_for_tenant(tenant_id: &str) -> Result<Vec<Invoice>>`

- Returns all matching invoices

## `pub fn mark_invoice_paid(&self, invoice_id: &str) -> Result<()>`

- Sets invoice status to `paid`
- Sets `paid_at` to now
- Clears `error` if set
- Logs activity as `(mark_invoice_paid, invoice_id)`

## `pub fn mark_invoice_attempted(&self, invoice_id: &str, error: &str) -> Result<()>`

- Sets `attempted_at` to now
- Updates `error` if provided
- Leaves status as `pending`
- Logs activity as `(mark_invoice_attempted, invoice_id)`

## `pub fn mark_invoice_sent(&self, invoice_id: &str) -> Result<()>`

- Sets `sent_at` to now
- Leaves status as `pending`
- Logs activity as `(mark_invoice_sent, invoice_id)`

## `pub fn mark_invoice_closed(&self, invoice_id: &str) -> Result<()>`

- Sets invoice status to `closed`
- Sets `closed_at` to now
- Logs activity as `(mark_invoice_closed, invoice_id)`

## `pub fn list_activity(&self, since: &i64) -> Result<Vec<Activity>>`

- Returns all activity occuring after `since`

## `pub fn list_activity_for_tenant(&self, tenant: &str, since: &i64) -> Result<Vec<Activity>>`

- Returns all activity occuring after `since` matching `tenant`

## `pub fn list_activity_for_relay(&self, relay_id: &str) -> Result<Vec<Activity>>`

- Returns all activity where `resource_type = 'relay'` and `resource_id = relay_id`
- Ordered newest-first

## `pub fn get_relay_plan_sats(&self, plan: &str) -> Result<i64>`

- Returns the monthly sats amount for a given plan id
- Uses `list_plans()` data for consistent pricing logic across API and billing
