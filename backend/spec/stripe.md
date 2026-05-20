# `pub struct Stripe`

A thin async wrapper around the subset of the Stripe REST API this service uses. It knows nothing about relays, tenants, or the database — it just speaks HTTP to Stripe and returns `serde_json::Value` (or small typed results). The domain logic that drives it lives in `spec/billing.md`.

Members:

- `secret_key: String` - Stripe API key, used as the bearer token and as the HMAC key for idempotency keys
- `webhook_secret: String` - secret for verifying Stripe webhook signatures
- `http: reqwest::Client`

All requests authenticate with `secret_key` via `Authorization: Bearer`. Write requests that must not be retried twice (customer/subscription/item creation, invoice payment) send a deterministic `Idempotency-Key` derived by HMAC-ing the operation name and its arguments with `secret_key`. Reconcile-to-desired-state writes (e.g. setting an item quantity) intentionally omit the idempotency key, since re-applying the same target is a no-op.

On any 4xx/5xx response, the wrapper reads the body and folds Stripe's JSON error payload (`error.message` / `error.type` / `error.code` / `error.param`) into the returned error so callers get an actionable message instead of a bare status line.

## `pub fn from_env() -> Self`

Reads `STRIPE_SECRET_KEY` and `STRIPE_WEBHOOK_SECRET` (both required) and constructs the client. Panics if either is missing or blank. This is what `Billing::new` calls.

## `pub fn new(secret_key: String, webhook_secret: String) -> Self`

Constructs the client with a fresh `reqwest::Client` from explicit keys (does not touch the environment).

## `pub async fn create_customer(&self, name: &str, tenant_pubkey: &str) -> Result<String>`

- `POST /v1/customers` with `name` and `metadata[tenant_pubkey]`
- Idempotent on `tenant_pubkey`
- Returns the new customer id; errors if it isn't a `cus_…` id

## `pub async fn get_subscription(&self, subscription_id: &str) -> Result<Option<Value>>`

- `GET /v1/subscriptions/:id`
- Returns `None` if Stripe responds `404` (so callers can recover from a stale subscription id), otherwise the subscription object

## `pub async fn create_subscription(&self, customer_id: &str, items: &BTreeMap<String, i64>) -> Result<(String, BTreeMap<String, String>)>`

- `POST /v1/subscriptions` with `collection_method: charge_automatically` and one `items[n][price]` / `items[n][quantity]` pair per entry
- Idempotent on the customer and the `(price, quantity)` set
- Returns the subscription id and a map from price id to the created subscription item id

## `pub async fn create_subscription_item(&self, subscription_id: &str, price_id: &str, quantity: i64) -> Result<String>`

- `POST /v1/subscription_items`
- Idempotent on `(subscription_id, price_id)`
- Returns the new subscription item id

## `pub async fn set_subscription_item_quantity(&self, item_id: &str, quantity: i64) -> Result<()>`

- `POST /v1/subscription_items/:id` with `quantity`
- No idempotency key (reconcile-to-target write)

## `pub async fn delete_subscription_item(&self, item_id: &str) -> Result<()>`

- `DELETE /v1/subscription_items/:id`

## `pub async fn cancel_subscription(&self, subscription_id: &str) -> Result<()>`

- `DELETE /v1/subscriptions/:id`

## `pub async fn list_invoices(&self, customer_id: &str) -> Result<Value>`

- `GET /v1/invoices?customer=…`
- Returns the `data` array

## `pub async fn get_invoice(&self, invoice_id: &str) -> Result<Value, InvoiceLookupError>`

- `GET /v1/invoices/:id`
- On a 4xx response, returns `InvoiceLookupError::StripeClient { status }` (callers usually surface this as a client error, e.g. `404` "no such invoice"); other failures are `InvoiceLookupError::Internal`
- Returns the full invoice object

## `pub async fn pay_invoice(&self, invoice_id: &str) -> Result<()>`

- `POST /v1/invoices/:id/pay` (retries collection using the customer's default payment method)
- Idempotent on `invoice_id`

## `pub async fn pay_invoice_out_of_band(&self, invoice_id: &str) -> Result<()>`

- `POST /v1/invoices/:id/pay` with `paid_out_of_band: true` — used when payment was collected over Lightning rather than through Stripe
- Idempotent on `invoice_id`

## `pub async fn preview_invoice(&self, customer_id: &str, subscription_id: Option<&str>) -> Result<Value>`

- `GET /v1/invoices/upcoming?customer=…[&subscription=…]`
- Used to validate proration when a subscription is downgraded

## `pub async fn has_payment_method(&self, customer_id: &str) -> Result<bool>`

- `GET /v1/payment_methods?customer=…&type=card`
- Returns whether the customer has at least one card on file

## `pub async fn create_portal_session(&self, customer_id: &str, return_url: Option<&str>) -> Result<String>`

- `POST /v1/billing_portal/sessions` with `customer` and optional `return_url`
- Returns the portal session URL

## `pub fn construct_event(&self, payload: &str, sig_header: &str) -> Result<Event>`

Verifies the `Stripe-Signature` header against `webhook_secret` and parses the body.

- Parse `t=` (timestamp) and `v1=` (signature) from the header
- Compute `HMAC-SHA256(webhook_secret, "{t}.{payload}")`, hex-encode it, and compare to `v1=`; error on mismatch
- Error if the timestamp is more than 300 seconds from now
- Returns the deserialized `Event` (`{ event_type, data: { object } }`)

# `pub enum InvoiceLookupError`

- `StripeClient { status: reqwest::StatusCode }` - Stripe returned a 4xx for an invoice lookup
- `Internal(anyhow::Error)` - any other failure

Implements `Display`/`Error` and `From<anyhow::Error>` / `From<reqwest::Error>` (both mapping to `Internal`).

# `pub struct Event` / `pub struct EventData`

The verified, parsed webhook event: `Event { event_type: String, data: EventData }`, `EventData { object: serde_json::Value }`.
