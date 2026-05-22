# `pub struct Stripe`

A thin async wrapper around the subset of the Stripe REST API this service uses. It knows nothing about relays, tenants, or the database — it just speaks HTTP to Stripe and returns small typed results. The domain logic that drives it lives in `spec/billing.md`, and the webhook dispatch lives in `spec/api.md`.

Members:

- `env: Env` - configuration; supplies the Stripe secret key (bearer token + idempotency HMAC key) and the webhook signing secret
- `http: reqwest::Client`

All requests authenticate with `env.stripe_secret_key` via `Authorization: Bearer`. Write requests that must not be retried twice (customer/subscription/item creation, invoice payment) send a deterministic `Idempotency-Key` derived by HMAC-ing the operation name and its arguments with the secret key. Reconcile-to-desired-state writes (e.g. setting an item quantity, deleting/canceling) intentionally omit the idempotency key, since re-applying the same target is a no-op.

On any 4xx/5xx response, the wrapper reads the body and folds Stripe's JSON error payload (`error.message` / `error.type` / `error.code` / `error.param`) into the returned error so callers get an actionable message instead of a bare status line.

## `pub fn new(env: &Env) -> Self`

Constructs the client with a fresh `reqwest::Client`, holding a clone of `env`. This is what `Billing::new` and `main` call.

## `pub async fn create_customer(&self, tenant_pubkey: &str, name: &str) -> Result<String>`

- `POST /v1/customers` with `name` and `metadata[tenant_pubkey]`
- Idempotent on `tenant_pubkey`
- Returns the new customer id

## `pub async fn get_subscription(&self, subscription_id: &str) -> Result<Option<StripeSubscription>>`

- `GET /v1/subscriptions/:id`
- Returns `None` if Stripe responds `404` (so callers can recover from a stale subscription id), otherwise the parsed `StripeSubscription`

## `pub async fn create_subscription(&self, customer_id: &str, items: &BTreeMap<String, i64>) -> Result<StripeSubscription>`

- `POST /v1/subscriptions` with `collection_method: charge_automatically` and one `items[n][price]` / `items[n][quantity]` pair per entry
- Idempotent on the customer and the `(price, quantity)` set
- Returns the created `StripeSubscription` (including its items)

## `pub async fn create_subscription_item(&self, subscription_id: &str, price_id: &str, quantity: i64) -> Result<()>`

- `POST /v1/subscription_items`
- Idempotent on `(subscription_id, price_id)`

## `pub async fn set_subscription_item_quantity(&self, item_id: &str, quantity: i64) -> Result<()>`

- `POST /v1/subscription_items/:id` with `quantity`
- No idempotency key (reconcile-to-target write)

## `pub async fn delete_subscription_item(&self, item_id: &str) -> Result<()>`

- `DELETE /v1/subscription_items/:id`

## `pub async fn cancel_subscription(&self, subscription_id: &str) -> Result<()>`

- `DELETE /v1/subscriptions/:id`

## `pub async fn list_invoices(&self, customer_id: &str) -> Result<Vec<StripeInvoice>>`

- `GET /v1/invoices?customer=…`
- Returns the parsed `data` array

## `pub async fn get_invoice(&self, invoice_id: &str) -> Result<Option<StripeInvoice>>`

- `GET /v1/invoices/:id`
- Returns `None` if Stripe responds `404`, otherwise the parsed `StripeInvoice`

## `pub async fn pay_invoice(&self, invoice_id: &str) -> Result<()>`

- `POST /v1/invoices/:id/pay` (retries collection using the customer's default payment method)
- Idempotent on `invoice_id`

## `pub async fn pay_invoice_out_of_band(&self, invoice_id: &str) -> Result<()>`

- `POST /v1/invoices/:id/pay` with `paid_out_of_band: true` — used when payment was collected over Lightning rather than through Stripe
- Idempotent on `invoice_id` (under a distinct key from `pay_invoice`)

## `pub async fn has_payment_method(&self, customer_id: &str) -> Result<bool>`

- `GET /v1/payment_methods?customer=…&type=card`
- Returns whether the customer has at least one card on file

## `pub async fn create_portal_session(&self, customer_id: &str, return_url: Option<&str>) -> Result<String>`

- `POST /v1/billing_portal/sessions` with `customer` and optional `return_url`
- Returns the Customer Portal session URL

## `pub fn get_webhook_event(&self, payload: &str, signature: &str) -> Result<StripeWebhookEvent>`

Verifies the `Stripe-Signature` header against `env.stripe_webhook_secret` and parses the body.

- Parse `t=` (timestamp) and `v1=` (signature) from the header
- Compute `HMAC-SHA256(secret, "{t}.{payload}")`, hex-encode it, and compare to `v1=`; error on mismatch
- Error if the timestamp is more than 300 seconds from now
- Returns the deserialized `StripeWebhookEvent`

# Typed results

- `StripeWebhookEvent { event_type: String, data: StripeWebhookEventData }`, `StripeWebhookEventData { object: serde_json::Value }` — the verified, parsed webhook event (`event_type` deserializes from the JSON `type` field)
- `StripeSubscription { id, status, items: Vec<StripeSubscriptionItem> }` (`items` flattened from Stripe's `{ data: [...] }` list)
- `StripeSubscriptionItem { id, price: StripePrice, quantity }` (`quantity` defaults to 1 when absent)
- `StripePrice { id }`
- `StripeInvoice { id, customer, status, amount_due, currency, period_start, period_end }` (the subset of invoice fields the API surfaces; `Serialize` + `Clone`)
