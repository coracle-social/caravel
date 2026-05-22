# `pub struct Api`

Api owns the HTTP interface: the shared application state, the router, NIP-98 authentication, and the authorization helpers. The route handlers themselves live in `crate::routes` (`routes/identity.rs`, `plans.rs`, `tenants.rs`, `relays.rs`, `invoices.rs`, `stripe.rs`) and use the response helpers in `spec/web.md`.

Members:

- `env: Env` - configuration (see `spec/env.md`); supplies the NIP-98 host check, admin pubkeys, encryption, etc.
- `query: Query`
- `command: Command`
- `billing: Billing`
- `stripe: Stripe`
- `robot: Robot`
- `infra: Infra`

Notes:

- Authentication is done using NIP-98, comparing the event's `u` tag to `env.server_host`, not the incoming request URL.
- The shared `Api` is wrapped in an `Arc` and handed to every handler as `State<Arc<Api>>`.
- A handler that requires an authenticated caller takes an `AuthedPubkey` extractor; handlers that omit it are anonymous.
- Each handler is responsible for authorization using `require_admin` or `require_admin_or_tenant`.
- Successful responses are `{ data, code: "ok" }`; error responses are `{ error, code }`, both with an appropriate HTTP status (see `spec/web.md`).

## `pub fn new(query, command, billing, stripe, robot, infra, env: &Env) -> Self`

- Stores the services and a clone of `env`

## `pub fn router(self) -> Router`

- Wraps `self` in an `Arc` and returns an `axum::Router` with the routes below as state-bearing routes

## `pub fn is_admin(&self, pubkey: &str) -> bool`

- Whether `pubkey` is in `env.server_admin_pubkeys`

## `pub fn require_admin(&self, authorized_pubkey: &str) -> Result<(), ApiError>`

- `Ok` if `authorized_pubkey` is an admin, otherwise a `403`

## `pub fn require_admin_or_tenant(&self, authorized_pubkey: &str, tenant_pubkey: &str) -> Result<(), ApiError>`

- `Ok` if `authorized_pubkey` is an admin or equals `tenant_pubkey`, otherwise a `403`

## `pub async fn get_tenant_or_404(&self, pubkey: &str) -> Result<Tenant, ApiError>`

- Looks up a tenant, returning `404` `not-found` if missing and `500` on a query error

## `pub async fn get_relay_or_404(&self, id: &str) -> Result<Relay, ApiError>`

- Looks up a relay, returning `404` `not-found` if missing and `500` on a query error

# Authentication

## `pub struct AuthedPubkey(pub String)`

An axum extractor (`FromRequestParts`) that authenticates a request via NIP-98 and yields the signer's pubkey. Adding it to a handler signature is what enforces "must be authenticated"; on failure the request is rejected with a `401`.

## `fn extract_auth_pubkey(&self, headers: &HeaderMap) -> Result<String, ApiError>` / `fn decode_nip98_pubkey(&self, headers) -> Result<String>`

- Parses the `Authorization` header, which must use the `Nostr ` scheme followed by a base64-encoded NIP-98 event
- Decodes and parses the event, requires kind `27235` (`HttpAuth`), and verifies its signature
- Requires the event's `u` tag to contain `env.server_host` (skipped when `server_host` is empty)
- Intentionally does **not** enforce exact request URL/method/query matching, and does **not** validate the `payload` tag/hash, `created_at` freshness window, or a replay nonce/cache
- This is a deliberate session-style tradeoff to reduce repeated signer prompts in the client
- Returns the signer pubkey (hex) when all checks pass; any failure surfaces as a `401`

Refer to https://github.com/nostr-protocol/nips/blob/master/98.md for details. Uses `nostr_sdk` functionality where possible.

# Routes

Handlers take `State<Arc<Api>>`, an optional `AuthedPubkey`, then path/query/body extractors, and return `ApiResult`.

--- Identity

## `get_identity` — `GET /identity`

- Authenticated (any signer)
- Side-effect-free: returns `{ pubkey, is_admin }`
- Clients must call `POST /tenants` before any tenant-scoped write

--- Plans

## `list_plans` — `GET /plans`

- No authentication required
- `data` is the list of plans from `query.list_plans`

## `get_plan` — `GET /plans/:id`

- No authentication required
- `data` is the plan matching `id`; `404` `not-found` if it doesn't exist

--- Tenants

## `list_tenants` — `GET /tenants`

- Admin only
- `data` is a list of `TenantResponse` (exposes `nwc_is_set: bool` instead of `nwc_url`)

## `create_tenant` — `POST /tenants`

- Authenticated (any signer); the target pubkey is the auth pubkey, no request body
- Idempotent: if a tenant already exists for the auth pubkey, return it without calling Stripe or writing to the DB
- Otherwise resolve a display name via `robot.fetch_nostr_name` (falling back to the first 8 chars of the pubkey), create a Stripe customer via `stripe.create_customer`, and create the tenant. No subscription is created yet — that happens when the first paid relay is added.
- On a unique-constraint race (`pubkey-exists`), re-fetch and return the existing tenant
- Always returns `200`; `data` is a `TenantResponse`

## `get_tenant` — `GET /tenants/:pubkey`

- Admin or matching tenant
- `data` is a `TenantResponse`

## `update_tenant` — `PUT /tenants/:pubkey`

- Admin or matching tenant
- Accepts an optional `nwc_url`: an empty string clears it, otherwise it is encrypted at rest via `env.encrypt`
- Updates the tenant via `command.update_tenant`
- `data` is the updated `TenantResponse`

## `list_tenant_relays` — `GET /tenants/:pubkey/relays`

- Admin or matching tenant
- `data` is the tenant's relays from `query.list_relays_for_tenant`

--- Relays

## `list_relays` — `GET /relays`

- Admin only
- `data` is all relays from `query.list_relays`

## `get_relay` — `GET /relays/:id`

- `404` `not-found` if the relay doesn't exist; then admin or relay owner
- `data` is the relay

## `list_relay_members` — `GET /relays/:id/members`

- Admin or relay owner
- For unsynced relays (`synced = 0`), returns an empty member list without calling zooid
- For synced relays, proxies the member list from zooid via `infra.list_relay_members`
- `data` is `{ members }`

## `create_relay` — `POST /relays`

- Admin or the `tenant` pubkey in the request body
- Generates the relay `id`/`schema`, validates and normalizes the relay via `prepare_relay`, and creates it via `command.create_relay`
- Duplicate subdomain → `422` `subdomain-exists`
- `data` is the relay; HTTP `201`

## `update_relay` — `PUT /relays/:id`

- `404` if missing; then admin or relay owner
- Applies the provided optional fields, then validates/normalizes via `prepare_relay`
- If the plan changes to one with a finite member limit and the current member count exceeds it, return `422` `member-limit-exceeded`
- Updates via `command.update_relay`; duplicate subdomain → `422` `subdomain-exists`
- `data` is the relay

## `list_relay_activity` — `GET /relays/:id/activity`

- `404` if missing; then admin or relay owner
- `data` is `{ activity }` from `query.list_activity_for_resource`

## `deactivate_relay` — `POST /relays/:id/deactivate`

- `404` if missing; then admin or relay owner
- If status is `delinquent`, return `400` `relay-is-delinquent`; if already `inactive`, return `400` `relay-is-inactive`
- Otherwise `command.deactivate_relay`; `data` is empty

## `reactivate_relay` — `POST /relays/:id/reactivate`

- `404` if missing; then admin or relay owner
- If status is `delinquent`, return `400` `relay-is-delinquent` (a delinquent relay must be resolved through payment, not reactivated by the user); if already `active`, return `400` `relay-is-active`
- Otherwise `command.activate_relay`; `data` is empty

--- Invoices

## `list_tenant_invoices` — `GET /tenants/:pubkey/invoices`

- Admin or matching tenant
- Looks up the tenant, then lists invoices from Stripe by `stripe_customer_id`
- `data` is a list of `StripeInvoice` objects: `{ id, customer, status, amount_due, currency, period_start, period_end }`

## `get_invoice` — `GET /invoices/:id`

- Fetches the invoice from Stripe (`404` `not-found` if it doesn't exist)
- Looks up the tenant by the invoice's `customer` (`404` if none), then authorizes admin or matching tenant
- Runs `billing.reconcile_invoice` (marks it paid if its bolt11 already settled out of band)
- `data` is the (possibly refreshed) `StripeInvoice`

## `get_lightning_invoice` — `GET /invoices/:id/bolt11`

- Fetches the invoice from Stripe (`404` if it doesn't exist) and the tenant by `customer` (`404` if none), then authorizes admin or matching tenant
- Runs `billing.reconcile_invoice`, then `billing.ensure_lightning_invoice` to get or (re)issue the bolt11 for the invoice's `amount_due`/`currency`
- `data` is the `LightningInvoice` (including its `bolt11`)

--- Stripe portal

## `create_stripe_session` — `GET /tenants/:pubkey/stripe/session`

- Admin or matching tenant; accepts an optional `return_url` query parameter
- Looks up the tenant and creates a Stripe Customer Portal session for its `stripe_customer_id`
- `data` is `{ url }` — the portal session URL

--- Stripe webhook

## `stripe_webhook` — `POST /stripe/webhook`

- No NIP-98 authentication — verified via the `Stripe-Signature` header over the raw body
- Reads the raw body and signature, verifies/parses the event via `stripe.get_webhook_event`, and dispatches to the handlers below
- Returns `200` on success, `400` (`webhook-error`) on verification/parse failure

# Webhook event handlers

Implemented in `routes/stripe.rs`. They translate verified Stripe events into domain actions, looking the tenant up by `stripe_customer_id` and ignoring events whose customer doesn't map to a tenant. Unknown event types are ignored.

## `invoice.created`

Attempts to pay a new subscription invoice (Stripe is pay-in-advance, so this fires immediately when a paid relay is added or a plan is upgraded). Skips `amount_due` of 0. Ensures a `LightningInvoice` exists, then in priority order:

1. **NWC auto-pay**: if the tenant has a `nwc_url`, run `billing.pay_invoice_nwc`. On success, done. On failure, record the error via `command.set_tenant_nwc_error`, log it, summarize it for the eventual DM, and fall through.
2. **Card on file**: if `stripe.has_payment_method`, do nothing — Stripe charges automatically for this attempt.
3. **Manual payment**: send a DM via `robot.send_dm` telling the tenant payment is due (including the summarized NWC error, if any), with a link to the app for manual Lightning payment.

## `invoice.paid`

- If the tenant has `past_due_at` set, clear it (`command.clear_tenant_past_due`) and reactivate each `delinquent` relay on a paid plan via `command.activate_relay`

## `invoice.payment_failed`

- If the tenant doesn't already have `past_due_at` set, set it (`command.set_tenant_past_due`) and DM the tenant that payment failed and their relays may be deactivated if unresolved

## `invoice.overdue`

- Mark every `active` relay on a paid plan `delinquent` (`command.mark_relay_delinquent`) and DM the tenant that their paid relays were deactivated for non-payment

## `customer.subscription.updated`

- If the subscription status is `canceled` or `unpaid`, clear `stripe_subscription_id` (`command.clear_tenant_subscription`) and mark every `active` paid relay `delinquent`

## `customer.subscription.deleted`

- Clear `stripe_subscription_id` (`command.clear_tenant_subscription`)

## `payment_method.attached`

- Retry Stripe collection (`stripe.pay_invoice`) for every `open` invoice with `amount_due > 0`, so invoices that were due before the card was added are charged immediately

# Helpers

## `prepare_relay(api: &Api, relay: Relay) -> Result<Relay, ApiError>`

- Validates `subdomain` against the allowed pattern and a reserved list (`api`, `admin`, `internal`) → `422` `invalid-subdomain`
- Validates that `plan` matches a known plan → `422` `invalid-plan`
- If the relay enables `blossom`/`livekit` but the selected plan doesn't include it → `422` `premium-feature`
- Normalizes the boolean relay flags to sane defaults

# `TenantResponse`

The tenant shape returned by tenant endpoints. Same as `Tenant` but replaces `nwc_url` with `nwc_is_set: bool` (true when a `nwc_url` is stored) and never exposes the stored URL: `{ pubkey, nwc_is_set, nwc_error, created_at, stripe_customer_id, stripe_subscription_id, past_due_at }`.
