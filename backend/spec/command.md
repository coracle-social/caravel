# `pub struct Command`

Command writes to the database.

Members:

- `pool: SqlitePool` - a sqlite connection pool
- `pub notify: broadcast::Sender<Activity>` - callers can subscribe via `command.notify.subscribe()`

Notes:

- Write methods that mutate tenants/relays are atomic and accompanied by an activity log entry of `(tenant, activity_type, resource_type, resource_id)`, run inside a single transaction via the `with_activity` helper.
- `insert_activity` builds and returns the `Activity` struct (resolving `tenant` from the resource — directly for `tenant` resources, by looking up `relay.tenant` for `relay` resources — and using `chrono::Utc::now()` for `created_at`).
- After each successful commit, the `Activity` is sent on the broadcast channel.
- The subscription/error/past-due setters and the lightning-invoice writes below intentionally do **not** log activity and write directly to the pool.

## `pub fn new(pool: SqlitePool) -> Self`

- Stores the pool and creates the broadcast channel

## `pub async fn create_tenant(&self, tenant: &Tenant) -> Result<()>`

- Creates tenant (writes `pubkey`, `nwc_url`, `created_at`, `stripe_customer_id`), may throw sqlite uniqueness error on pubkey
- Logs activity as `(create_tenant, tenant, pubkey)`

## `pub async fn update_tenant(&self, tenant: &Tenant) -> Result<()>`

- Updates the tenant's `nwc_url`
- Logs activity as `(update_tenant, tenant, pubkey)`

## `pub async fn create_relay(&self, relay: &Relay) -> Result<()>`

- Creates relay with status `active` and `synced = 0`, may throw sqlite uniqueness error on subdomain
- Logs activity as `(create_relay, relay, id)`

## `pub async fn update_relay(&self, relay: &Relay) -> Result<()>`

- Updates relay (all mutable fields), resets `synced = 0` so it re-syncs to zooid; may throw sqlite uniqueness error on subdomain
- Logs activity as `(update_relay, relay, id)`

## `pub async fn deactivate_relay(&self, relay: &Relay) -> Result<()>`

- Sets relay status to `inactive` and `synced = 0`
- Logs activity as `(deactivate_relay, relay, id)`
- Used for user/admin-initiated deactivation only

## `pub async fn mark_relay_delinquent(&self, relay: &Relay) -> Result<()>`

- Sets relay status to `delinquent` and `synced = 0`
- Logs activity as `(mark_relay_delinquent, relay, id)`
- Used exclusively by the billing system when a relay's subscription becomes past due
- `delinquent` relays are automatically reactivated via `activate_relay` when payment is received

## `pub async fn activate_relay(&self, relay: &Relay) -> Result<()>`

- Sets relay status to `active` and `synced = 0`
- Logs activity as `(activate_relay, relay, id)`

## `pub async fn fail_relay_sync(&self, relay: &Relay, sync_error: String) -> Result<()>`

- Sets `synced = 0` and `sync_error` on the relay
- Logs activity as `(fail_relay_sync, relay, id)`

## `pub async fn complete_relay_sync(&self, relay_id: &str) -> Result<()>`

- Sets `synced = 1`, clears `sync_error`
- Logs activity as `(complete_relay_sync, relay, id)`

## `pub async fn set_tenant_subscription(&self, pubkey: &str, stripe_subscription_id: &str) -> Result<()>`

- Sets `stripe_subscription_id` on the tenant
- Does not log activity

## `pub async fn clear_tenant_subscription(&self, pubkey: &str) -> Result<()>`

- Sets `stripe_subscription_id = null` on the tenant
- Does not log activity

## `pub async fn set_tenant_nwc_error(&self, pubkey: &str, error: &str) -> Result<()>`

- Sets `nwc_error` on the tenant
- Does not log activity

## `pub async fn clear_tenant_nwc_error(&self, pubkey: &str) -> Result<()>`

- Sets `nwc_error = null` on the tenant
- Does not log activity

## `pub async fn set_tenant_past_due(&self, pubkey: &str) -> Result<()>`

- Sets `past_due_at` to the current timestamp
- Does not log activity

## `pub async fn clear_tenant_past_due(&self, pubkey: &str) -> Result<()>`

- Sets `past_due_at = null` on the tenant
- Does not log activity

## `pub async fn insert_lightning_invoice(&self, stripe_invoice_id: &str, tenant_pubkey: &str, bolt11: &str, expires_at: i64) -> Result<Option<LightningInvoice>>`

- Upserts the `pending` bolt11 row for a Stripe invoice, returning the resulting row
- On conflict the stored `bolt11`/`expires_at` are replaced (this is how an expired invoice is regenerated), **except** once the invoice is `paid`: the `status = 'pending'` guard makes the update a no-op and `None` is returned so the caller can fall back to reading the settled row
- Does not log activity

## `pub async fn mark_lightning_invoice_paid(&self, stripe_invoice_id: &str, method: &str) -> Result<()>`

- Marks a `pending` invoice `paid`, recording `paid_method = method` (`nwc` or `manual`)
- The `status = 'pending'` guard makes this idempotent and first-writer-wins: a later reconcile won't clobber the method recorded by whoever settled it first
- Does not log activity
