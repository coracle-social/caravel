# `pub struct Query`

Query reads from the database.

Members:

- `pool: SqlitePool` - a sqlite connection pool

## `pub fn new(&self, pool: SqlitePool) -> Self`

- Assigns pool to self

## `pub fn list_tenants(&self) -> Result<Vec<Tenant>>`

- Returns all tenants

## `pub fn get_tenant(&self, pubkey: &str) -> Result<Tenant>`

- Returns matching tenant

## `pub fn list_plans() -> Vec<Plan>`

- Returns the hardcoded relay plans used by the system (`free`, `basic`, `growth`)
- This is the source of truth for plan metadata exposed via API

## `pub fn list_relays(&self) -> Result<Vec<Relay>>`

- Returns all relays

## `pub fn list_relays_for_tenant(&self, tenant_id: &str) -> Result<Vec<Relay>>`

- Returns all relays belonging to the given tenant

## `pub fn get_relay(&self, id: &str) -> Result<Relay>`

- Returns matching relay

## `pub fn max_activity_at(&self) -> Result<i64>`

- Returns the maximum `created_at` value from the activity table, or 0 if empty
- Used by infra to initialize the since guard on startup

## `pub fn list_activity(&self, since: &i64) -> Result<Vec<Activity>>`

- Returns all activity occuring after `since`

## `pub fn list_activity_for_relay(&self, relay_id: &str) -> Result<Vec<Activity>>`

- Returns all activity where `resource_type = 'relay'` and `resource_id = relay_id`
- Ordered newest-first
