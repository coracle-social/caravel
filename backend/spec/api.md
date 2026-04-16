# `pub struct Api`

Api manages the HTTP interface for the application

Members:

- `host: String` - the hostname of the service for checking NIP 98 auth, from `HOST`
- `admins: Vec<String>` - a list of admin pubkeys from `ADMINS`
- `query: Query`
- `command: Command`
- `billing: Billing`

Notes:

- Authentication is done using NIP 98 comparing `u` to `self.host`, not the incoming request
- Each route is responsible for authorization using `self.require_admin` or `self.require_admin_or_tenant`
- Successful API responses should be of the form `{data, code: "ok"}` with an appropriate http status code.
- Unsuccessful API responses should be of the form `{error, code}` with an appropriate http status code. `code` is a short error code (e.g. `duplicate-subdomain`) and `error` is a human-readable error message.

## `pub fn new() -> Self`

- Reads environment and populates members

## `pub fn router(&self) -> Result<()>`

- Returns an `axum::Router`

--- Plan routes

## `async fn list_plans(...) -> Response`

- Serves `GET /plans`
- No authentication required
- Return `data` is a list of plan structs from `Query::list_plans`

## `async fn get_plan(...) -> Response`

- Serves `GET /plans/:id`
- No authentication required
- Return `data` is a single plan struct matching `id`
- If plan does not exist, return `404` with `code=not-found`

--- Identity routes

## `async fn get_identity(...) -> Response`

- Serves `GET /identity`
- Authorizes anyone, but must be authorized
- If a tenant for the identity doesn't exist:
  - Call the Stripe API to create a new customer
  - Create a new tenant using `command.create_tenant` with payload and `stripe_customer_id`. No subscription is created yet — that happens when the first relay is added.
- Return `data` is an `Identity` struct

--- Tenant routes

## `async fn list_tenants(...) -> Response`

- Serves `GET /tenants`
- Authorizes admin only
- Return `data` is a list of tenant structs from `query.list_tenants`

## `async fn get_tenant(...) -> Response`

- Serves `GET /tenants/:pubkey`
- Authorizes admin or matching tenant
- Return `data` is a single tenant struct from `query.get_tenant`

## `async fn update_tenant(...) -> Response`

- Serves `PUT /tenants/:pubkey`
- Authorizes admin or matching tenant
- Updates tenant using `command.update_tenant`
- Return `data` is the updated tenant struct

## `async fn list_tenant_relays(...) -> Response`

- Serves `GET /tenants/:pubkey/relays`
- Authorizes admin or matching tenant
- Return `data` is a list of relay structs from `query.list_relays_for_tenant`

--- Relay routes

## `async fn list_relays(...) -> Response`

- Serves `GET /relays`
- Authorizes admin only
- Return `data` is a list of relay structs from `query.list_relays`

## `async fn get_relay(...) -> Response`

- Serves `GET /relays/:id`
- Authorizes admin or relay owner
- Return `data` is a single relay struct from `query.get_relay`

## `async fn create_relay(...) -> Response`

- Serves `POST /relays`
- Authorizes admin or matching tenant pubkey in request body
- Validates/prepares the relay data to be saved using `prepare_relay`
- Creates a new relay using `command.create_relay`
- If relay is a duplicate by subdomain, return a `422` with `code=subdomain-exists`
- Return `data` is a single relay struct. Use HTTP `201`.

## `async fn update_relay(...) -> Response`

- Serves `PUT /relays/:id`
- Authorizes admin or relay owner
- Validates/prepares the relay data to be saved using `prepare_relay`
- Updates the given relay using `command.update_relay`
- If relay is a duplicate by subdomain, return a `422` with `code=subdomain-exists`
- Return `data` is a single relay struct.

## `async fn list_relay_activity(...) -> Response`

- Serves `GET /relays/:id/activity`
- Authorizes admin or relay owner
- Get activity from `query.list_activity_for_relay`
- Return `data` is `{activity}`

## `async fn deactivate_relay(...) -> Response`

- Serves `POST /relays/:id/deactivate`
- Authorizes admin or relay owner
- If relay is already inactive, return a `400` with `code=relay-is-inactive`
- Call `command.deactivate_relay`
- Return `data` is empty

## `async fn reactivate_relay(...) -> Response`

- Serves `POST /relays/:id/reactivate`
- Authorizes admin or relay owner
- If relay is already active, return a `400` with `code=relay-is-active`
- Call `command.activate_relay`
- Return `data` is empty

--- Invoice routes

## `async fn list_tenant_invoices(...) -> Response`

- Serves `GET /tenants/:pubkey/invoices`
- Authorizes admin or matching tenant
- Looks up tenant by pubkey, fetches invoices from Stripe API using `stripe_customer_id`
- Return `data` is a list of Stripe invoice objects: `{ id, status, amount_due, currency, hosted_invoice_url, period_start, period_end }`

## `async fn get_invoice(...) -> Response`

- Serves `GET /invoices/:id`
- Fetches invoice from Stripe API by ID
- Looks up tenant by the invoice's `customer` field, authorizes admin or matching tenant
- Return `data` is a single Stripe invoice object
- If invoice does not exist, return `404` with `code=not-found`

## `async fn get_invoice_bolt11(...) -> Response`

- Serves `GET /invoices/:id/bolt11`
- Fetches invoice from Stripe API by ID
- Looks up tenant by the invoice's `customer` field, authorizes admin or matching tenant
- If invoice `status` is not `open`, return `400` with `code=invoice-not-open`
- Creates a bolt11 Lightning invoice for the invoice's `amount_due` using `billing.create_bolt11(amount_due)`
- Return `data` is `{ bolt11 }`

--- Stripe session route

## `async fn create_stripe_session(...) -> Response`

- Serves `GET /tenants/:pubkey/stripe/session`
- Authorizes admin or matching tenant
- Looks up tenant by pubkey
- Creates a Stripe Customer Portal session for the tenant's `stripe_customer_id`
- Return `data` is `{ url }` — the portal session URL

--- Stripe webhook route

## `async fn stripe_webhook(...) -> Response`

- Serves `POST /stripe/webhook`
- No NIP-98 authentication — uses Stripe signature verification instead
- Reads raw request body and `Stripe-Signature` header
- Calls `billing.handle_webhook(payload, signature)`
- Returns `200` on success, `400` on signature verification failure

--- Utilities

## `extract_auth_pubkey(&self, headers: &HeaderMap) -> Result<String>`

- Parses `Authorization` header
- Validates event kind (`27235`) and signature using `nostr_sdk`
- Validates event `u` contains configured `HOST`
- Intentionally does **not** enforce exact request URL/method/query matching
- Intentionally does **not** validate `payload` tag/hash, `created_at` freshness window, or replay nonce/cache
- This is a deliberate session-style tradeoff to reduce repeated signer prompts in the client
- Returns pubkey if header all checks pass

Refer to https://github.com/nostr-protocol/nips/blob/master/98.md for details. Use `nostr_sdk` functionality where possible.

## `require_admin(&self, authorized_pubkey: &str)`

- Checks whether `authorized_pubkey` is in `self.admins`. If not, returns an forbidden error

## `require_admin_or_tenant(&self, authorized_pubkey: &str, tenant_pubkey: &str)`

- Checks whether `authorized_pubkey` is an admin or matches `tenant_pubkey`

## `prepare_relay(&self, relay: Relay) -> anyhow::Result<Relay>`

- Validate `subdomain`
- Validate that `plan` matches a known plan id from `Query::list_plans`
- If selected `plan` does not include `blossom` and `blossom` is enabled, return `premium-feature`
- If selected `plan` does not include `livekit` and `livekit` is enabled, return `premium-feature`
- Populate `schema` if not already set
- Populate missing fields using reasonable defaults
