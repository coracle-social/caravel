# `pub struct Api`

Api manages the HTTP interface for the application

Members:

- `host: String` - the hostname of the service for checking NIP 98 auth, from `HOST`
- `port: u16` - a port to run the server on from `PORT`
- `admins: Vec<String>` - a list of admin pubkeys from `ADMINS`
- `origins: Vec<String>` - to be used in CORS headers, from `ALLOW_ORIGINS`
- `repo: Repo`

Notes:

- Authentication is done using NIP 98 comparing `u` to `self.host`, not the incoming request
- Each route is responsible for authorization using `self.require_admin` or `self.require_admin_or_tenant`
- Successful API responses should be of the form `{data, code: "ok"}` with an appropriate http status code.
- Unsuccessful API responses should be of the form `{error, code}` with an appropriate http status code. `code` is a short error code (e.g. `duplicate-subdomain`) and `error` is a human-readable error message.

## `pub fn new() -> Self`

- Reads environment and populates members

## `pub fn serve(&self) -> Result<()>`

- Initializes an `axum::Router`
- Adds CORS middleware based on `origins`
- Calls `axum::serve` with a listener

--- Plan routes

## `async fn list_plans(...) -> Response`

- Serves `GET /plans`
- No authentication required
- Return `data` is a list of plan structs from `Repo::list_plans`

## `async fn get_plan(...) -> Response`

- Serves `GET /plans/:id`
- No authentication required
- Return `data` is a single plan struct matching `id`
- If plan does not exist, return `404` with `code=not-found`

--- Identity routes

## `async fn get_identity(...) -> Response`

- Serves `GET /identity`
- Authorizes anyone, but must be authorized
- If a tenant for the identity doesn't exist, one is created
- Return `data` is an `Identity` struct

--- Tenant routes

## `async fn list_tenants(...) -> Response`

- Serves `GET /tenants`
- Authorizes admin only
- Return `data` is a list of tenant structs from `repo.list_tenants`

## `async fn get_tenant(...) -> Response`

- Serves `GET /tenants/:pubkey`
- Authorizes admin or matching tenant
- Return `data` is a single tenant struct from `repo.get_tenant`

## `async fn list_tenant_relays(...) -> Response`

- Serves `GET /tenants/:pubkey/relays`
- Authorizes admin or matching tenant
- Return `data` is a list of relay structs from `repo.list_relays_for_tenant`

## `async fn list_tenant_invoices(...) -> Response`

- Serves `GET /tenants/:pubkey/invoices`
- Authorizes admin or matching tenant
- Return `data` is a list of invoice structs from `repo.list_invoices_for_tenant`

## `async fn update_tenant_billing(...) -> Response`

- Serves `PUT /tenants/:pubkey/billing`
- Authorizes admin or matching tenant
- Updates tenant billing NWC URL using `repo.update_tenant_nwc_url`
- Return `data` is the submitted billing payload

--- Relay routes

## `async fn list_relays(...) -> Response`

- Serves `GET /relays`
- Authorizes admin only
- Return `data` is a list of relay structs from `repo.list_relays`

## `async fn get_relay(...) -> Response`

- Serves `GET /relays/:id`
- Authorizes admin or relay owner
- Return `data` is a single relay struct from `repo.get_relay`

## `async fn create_relay(...) -> Response`

- Serves `POST /relays`
- Authorizes admin or matching tenant pubkey in request body
- Validates/prepares the relay data to be saved using `prepare_relay`
- Creates a new relay using `repo.create_relay`
- If relay is a duplicate by subdomain, return a `422` with `code=subdomain-exists`
- Return `data` is a single relay struct. Use HTTP `201`.

## `async fn update_relay(...) -> Response`

- Serves `PUT /relays/:id`
- Authorizes admin or relay owner
- Validates/prepares the relay data to be saved using `prepare_relay`
- Updates the given relay using `repo.update_relay`
- If relay is a duplicate by subdomain, return a `422` with `code=subdomain-exists`
- Return `data` is a single relay struct.

## `async fn list_relay_activity(...) -> Response`

- Serves `GET /relays/:id/activity`
- Authorizes admin or relay owner
- Return `data` is a list of activity structs from `repo.list_activity_for_relay`

## `async fn deactivate_relay(...) -> Response`

- Serves `POST /relays/:id/deactivate`
- Authorizes admin or relay owner
- Deactivates relay using `repo.deactivate_relay`
- Return `data` is empty

--- Invoice routes

## `async fn list_invoices(...) -> Response`

- Serves `GET /invoices`
- Authorizes admin only
- Return `data` is a list of invoice structs from `repo.list_invoices`

## `async fn get_invoice(...) -> Response`

- Serves `GET /invoices/:id`
- Authorizes admin or invoice owner
- Return `data` is a single invoice struct from `repo.get_invoice`

--- Utilities

## `extract_auth_pubkey(&self, headers: &HeaderMap) -> Result<String>`

- Parses `Authorization` header
- Validates event kind and signature using `nostr_sdk`
- Validates event `u` against `HOST` (not the request path. Non-standard, but correct)
- Does not validate `method` tag
- Returns pubkey if header all checks pass

Refer to https://github.com/nostr-protocol/nips/blob/master/98.md for details. Use `nostr_sdk` functionality where possible.

## `require_admin(&self, authorized_pubkey: &str)`

- Checks whether `authorized_pubkey` is in `self.admins`. If not, returns an forbidden error

## `require_admin_or_tenant(&self, authorized_pubkey: &str, tenant_pubkey: &str)`

- Checks whether `authorized_pubkey` is an admin or matches `tenant_pubkey`

## `prepare_relay(&self, relay: Relay) -> anyhow::Result<Relay>`

- Validate `subdomain`
- If `plan` is free and `blossom` is enabled, return `premium-feature`
- If `plan` is free and `livekit` is enabled, return `premium-feature`
- Populate `schema` if not already set
- Populate missing fields using reasonable defaults
