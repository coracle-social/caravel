# `pub struct Command`

Command writes to the database.

Members:

- `pool: SqlitePool` - a sqlite connection pool
- `pub notify: broadcast::Sender<Activity>` - callers can subscribe via `command.notify.subscribe()`

Notes:

- All public write methods should be atomic
- All writes should be accompanied by an activity log entry of `(tenant, activity_type, resource_type, resource_id)`
- `insert_activity` builds and returns the `Activity` struct (using `chrono::Utc::now()` for `created_at`)
- After each successful commit, sends the `Activity` on the broadcast channel

## `pub fn new(&self, pool: SqlitePool) -> Self`

- Assigns pool to self
- Creates the broadcast channel

## `pub fn create_tenant(&self, tenant: &Tenant) -> Result<()>`

- Creates tenant, may throw sqlite uniqueness error on pubkey
- Logs activity as `(create_tenant, tenant_id)`

## `pub fn update_tenant(&self, tenant: &Tenant) -> Result<()>`

- Updates tenant
- Logs activity as `(update_tenant, tenant_id)`

## `pub fn create_relay(&self, relay: &Relay) -> Result<()>`

- Creates relay, may throw sqlite uniqueness error on subdomain
- Sets relay status to `active`
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

- Sets `sync_error` on the relay
- Logs activity as `(fail_relay_sync, relay_id)`

## `pub fn complete_relay_sync(&self, relay_id: &str) -> Result<()>`

- Sets `synced = 1`, clears `sync_error`
- Logs activity as `(complete_relay_sync, relay_id)`

## `pub fn delete_relay_subscription_item(&self, relay_id: &str) -> Result<()>`

- Sets `stripe_subscription_item_id = null`
- Does not log activity

## `pub fn set_relay_subscription_item(&self, relay_id: &str, stripe_subscription_item_id: &str) -> Result<()>`

- Sets `stripe_subscription_item_id`
- Does not log activity
