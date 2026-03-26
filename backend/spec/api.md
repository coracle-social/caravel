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
- Each route is responsible for authorization using `self.is_admin(pubkey)` or `self.is_tenant(authorized_pubkey, tenant_pubkey)`
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
- Return `data` is a list of plan structs from `Repo::list_plans`

## `async fn get_plan(...) -> Response`

- Serves `GET /plans/:id`
- Return `data` is a single plan struct matching `id`
- If plan does not exist, return `404` with `code=not-found`

--- Tenant routes

## `async fn list_tenants(...) -> Response`

- Serves `GET /tenants`
- Authorizes admin only
- Return `data` is a list of tenant structs from `repo.list_tenants`

## `async fn get_tenant(...) -> Response`

- Serves `GET /tenants/:pubkey`
- Authorizes admin or matching tenant
- Return `data` is a single tenant struct from `repo.get_tenant`

## `async fn create_tenant(...) -> Response`

- Serves `POST /tenants`
- Authorizes anyone, but must be authorized
- Creates a new tenant using `repo.create_tenant` based on the authorized pubkey
- If tenant is a duplicate, return a `422` with `code=pubkey-exists`
- Return `data` is a single tenant struct. Use HTTP `201`.

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

# Utility functions

## `extract_auth_pubkey(headers: &HeaderMap, method: &Method, uri: &Uri) -> Result<String>`

- Parses `Authorization` header
- Validates event kind and signature using `nostr_sdk`
- Validates event `u` and `method` tags against parameters
- Returns pubkey if header is valid

Refer to https://github.com/nostr-protocol/nips/blob/master/98.md for details. Use `nostr_sdk` functionality where possible.

## `prepare_relay(relay: Relay) -> anyhow::Result<Relay>`

- Validate `subdomain`
- If `plan` is free and `blossom` is enabled, return `premium-feature`
- If `plan` is free and `livekit` is enabled, return `premium-feature`
- Populate `schema` if not already set
- Populate missing fields using reasonable defaults
