# `pub struct Query`

Query reads from the database.

Members:

- `pool: SqlitePool` - a sqlite connection pool
- `env: Env` - configuration; used to fill in plan `stripe_price_id`s from `STRIPE_PRICE_*`

## `pub fn new(pool: SqlitePool, env: &Env) -> Self`

- Stores the pool and a clone of `env`

## `pub fn list_plans(&self) -> Vec<Plan>`

- Returns the hardcoded relay plans used by the system (`free`, `basic`, `growth`)
- The `basic`/`growth` `stripe_price_id`s come from `env` (`stripe_price_basic` / `stripe_price_growth`); `free` has none
- This is the source of truth for plan metadata exposed via API

## `pub fn get_plan(&self, plan_id: &str) -> Option<Plan>`

- Returns the plan matching `plan_id`, if any

## `pub fn is_paid_plan(&self, plan_id: &str) -> bool`

- Returns whether `plan_id` is a known plan with `amount > 0`

## `pub async fn list_tenants(&self) -> Result<Vec<Tenant>>`

- Returns all tenants

## `pub async fn get_tenant(&self, pubkey: &str) -> Result<Option<Tenant>>`

- Returns the matching tenant, or `None` if not found

## `pub async fn get_tenant_by_stripe_customer_id(&self, stripe_customer_id: &str) -> Result<Option<Tenant>>`

- Returns the tenant matching the given `stripe_customer_id`, or `None`

## `pub async fn list_relays(&self) -> Result<Vec<Relay>>`

- Returns all relays

## `pub async fn list_relays_pending_sync(&self) -> Result<Vec<Relay>>`

- Returns all relays where `synced = 0` or `sync_error` is non-empty
- Used by `Infra` to reconcile relays that still need to be pushed to zooid

## `pub async fn list_relays_for_tenant(&self, tenant_id: &str) -> Result<Vec<Relay>>`

- Returns all relays belonging to the given tenant

## `pub async fn get_relay(&self, id: &str) -> Result<Option<Relay>>`

- Returns the matching relay, or `None` if not found

## `pub async fn get_lightning_invoice(&self, stripe_invoice_id: &str) -> Result<Option<LightningInvoice>>`

- Returns the `lightning_invoice` row for the given Stripe invoice, or `None`

## `pub async fn list_activity_for_resource(&self, resource_id: &str) -> Result<Vec<Activity>>`

- Returns all activity where `resource_id = resource_id`
- Ordered newest-first

## `pub async fn get_latest_activity_for_resource_and_type(&self, resource_id: &str, activity_type: &str) -> Result<Option<Activity>>`

- Returns the most recent activity for `resource_id` with the given `activity_type`, or `None`
- Used by `Infra` to decide whether a relay has ever completed a sync
