# `pub struct Billing`

Billing encapsulates the domain logic for synchronizing a tenant's Stripe subscription with their relays and for collecting Stripe invoices over Lightning (NWC auto-pay and manual payment).

It owns the domain logic only: Stripe REST calls go through `Stripe` (see `spec/stripe.md`), NWC wallet operations through `Wallet` (see `spec/wallet.md`), and fiat → msats conversion through `bitcoin` (see `spec/bitcoin.md`). The Stripe webhook dispatch that calls into `Billing` lives in `spec/api.md`.

Members:

- `stripe: Stripe` - Stripe REST wrapper (see `spec/stripe.md`)
- `wallet: Wallet` - the system NWC wallet, used to issue and look up bolt11 invoices (see `spec/wallet.md`)
- `query: Query`
- `command: Command`
- `env: Env` - used to decrypt a tenant's stored `nwc_url`

## `pub fn new(query: Query, command: Command, env: &Env) -> Self`

- Builds `stripe` via `Stripe::new(env)` and `wallet` via `Wallet::from_url(&env.robot_wallet)`
- Panics if `ROBOT_WALLET` is not a valid NWC URL

## `pub async fn start(self)`

- Subscribes to `command.notify.subscribe()`
- Runs a full reconcile (`reconcile_subscriptions("startup")`) before entering the loop
- Loops on `rx.recv()`, calling `handle_activity` for each `Activity`
- On `Lagged`, runs a full reconcile (`reconcile_subscriptions("lagged")`); on `Closed`, exits

## `async fn reconcile_subscriptions(&self, source: &str)`

- Calls `reconcile_subscription` for every tenant, logging (but not aborting on) per-tenant errors

## `async fn handle_activity(&self, activity: &Activity)`

- On `create_relay`, `update_relay`, `activate_relay`, `deactivate_relay`, `fail_relay_sync`, or `complete_relay_sync`: resolve the tenant named by the activity (skip if it no longer exists) and call `reconcile_subscription`

## `async fn reconcile_subscription(&self, tenant: &Tenant)`

Reconciles a tenant's single Stripe subscription with the set of relays that should be billed. Only paid (non-free) relays interact with Stripe; free-only tenants have no subscription. Idempotent.

Stripe forbids two subscription items on the same subscription from sharing a price, so billing is modeled as **one subscription item per plan (price), with `quantity` equal to the number of the tenant's `active` relays on that plan**. The relay → item mapping is fully derivable from `relay.plan → plan.stripe_price_id` and the live subscription's items, so nothing is persisted on the relay.

Stripe uses **pay-in-advance** by default: when a subscription is first created, Stripe immediately generates an open invoice for the current period. The `invoice.created` webhook fires shortly after and the webhook handler attempts payment (see `spec/api.md`).

- Build the desired state via `get_quantity_by_price_id`.
- **No relays to bill** (desired state empty): `ensure_subscription_is_inactive` and return.
- Otherwise `ensure_subscription_is_active` to resolve/create the subscription, then `ensure_subscription_items` to sync its items.

## `async fn get_quantity_by_price_id(&self, tenant: &Tenant) -> Result<BTreeMap<String, i64>>`

- For each `active` relay whose plan has a `stripe_price_id`, count relays per price. Returns the price → quantity map.

## `async fn get_subscription(&self, tenant: &Tenant) -> Result<Option<StripeSubscription>>`

- If the tenant has a `stripe_subscription_id`, fetch it from Stripe (`None` if Stripe 404s)
- If the fetched subscription's status is `canceled`/`incomplete_expired`, call `command.clear_tenant_subscription` and return `None`

## `async fn ensure_subscription_is_active(&self, tenant, quantity_by_price_id) -> Result<StripeSubscription>`

- Returns the existing subscription if there is one
- Otherwise creates a Stripe subscription with the desired items (Stripe rejects an itemless subscription) and saves the id via `command.set_tenant_subscription`

## `async fn ensure_subscription_is_inactive(&self, tenant: &Tenant)`

- If the tenant still has a live subscription, cancel it via Stripe and call `command.clear_tenant_subscription`

## `async fn ensure_subscription_items(&self, subscription, quantity_by_price_id)`

- For each desired `(price, quantity)`: update the matching item's quantity if it differs, otherwise create the item
- Delete any existing item whose price no longer appears in the desired state

## `pub async fn ensure_lightning_invoice(&self, stripe_invoice_id: &str, tenant_pubkey: &str, amount_due: i64, currency: &str) -> Result<LightningInvoice>`

- Returns the existing `lightning_invoice` row if it is already `paid`, or still `pending` and not expired
- Otherwise converts `amount_due` to msats via `bitcoin::fiat_to_msats`, issues a fresh bolt11 (1 hour expiry) on the system wallet, and upserts it via `command.insert_lightning_invoice` (re-reading the stored row if the upsert was a no-op because the invoice was already paid)

## `pub async fn pay_invoice_nwc(&self, tenant: &Tenant, invoice: &LightningInvoice) -> Result<()>`

Pays a Lightning invoice from the tenant's own wallet.

- Decrypt the tenant's `nwc_url` (`env.decrypt`) and build a tenant `Wallet`
- Pay `invoice.bolt11` from the tenant wallet
- On success, `settle_invoice(..., "nwc")`
- On a pay error, the payment may still have landed before the response was lost: check `wallet.is_settled(invoice.bolt11)` on the system wallet and `settle_invoice(..., "nwc")` if it settled; otherwise return the pay error

## `pub async fn reconcile_invoice(&self, invoice: &StripeInvoice) -> Result<StripeInvoice>`

Catches an out-of-band Lightning payment we never recorded (e.g. the user paid but the frontend failed to notify us). Meant to run before presenting a payable invoice so we never hand back one that's already been paid.

- If the invoice is not `open`, or has no `lightning_invoice` row, return it unchanged
- If its bolt11 has settled on the system wallet, `settle_invoice(..., "manual")` and return the re-fetched (now paid) Stripe invoice; fall back to the original snapshot if Stripe momentarily 404s
- On any settlement-lookup failure, log and return the invoice unchanged

## `async fn settle_invoice(&self, stripe_invoice_id: &str, tenant_pubkey: &str, method: &str)`

- `command.mark_lightning_invoice_paid(stripe_invoice_id, method)` (first-writer-wins)
- `stripe.pay_invoice_out_of_band(stripe_invoice_id)` (idempotent)
- `command.clear_tenant_nwc_error(tenant_pubkey)`
