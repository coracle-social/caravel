# `pub struct Billing`

Billing encapsulates logic related to synchronizing state with Stripe, processing payments via NWC, and managing subscription lifecycle.

It owns the domain logic only: every raw Stripe REST call goes through the `Stripe` wrapper (see `spec/stripe.md`), and every Lightning (NWC) wallet operation and the fiat-minor-units → millisatoshi conversion go through the helpers in `spec/bitcoin.md`.

Members:

- `stripe: Stripe` - thin wrapper around the Stripe REST API (see `spec/stripe.md`), built from `STRIPE_SECRET_KEY` / `STRIPE_WEBHOOK_SECRET`
- `bitcoin: Bitcoin` - the Bitcoin-facing config: system NWC wallet (`NWC_URL`) plus the BTC price-feed HTTP client (see `spec/bitcoin.md`)
- `query: Query`
- `command: Command`
- `robot: Robot`

## `pub fn new(query: Query, command: Command, robot: Robot) -> Self`

- Builds `stripe` via `Stripe::from_env()` and `bitcoin` via `Bitcoin::from_env()`
- Panics if `STRIPE_SECRET_KEY` or `STRIPE_WEBHOOK_SECRET` is missing/empty (`NWC_URL` is optional)

## `pub fn start(&self)`

- Subscribes to `command.notify.subscribe()`
- On `create_relay`, `update_relay`, `activate_relay`, `deactivate_relay`, `fail_relay_sync`, and `complete_relay_sync`: resolve the relay named by the activity (skip if it no longer exists) and reconcile its tenant via `sync_tenant_subscription`.
- The startup/lagged reconcile loop calls `sync_tenant_subscription` for every tenant.

## `fn sync_tenant_subscription(&self, tenant_pubkey: &str)`

Reconciles a tenant's single Stripe subscription with the set of relays that should be billed. Only paid (non-free) relays interact with Stripe. Free-only tenants have no subscription. Must be idempotent.

Stripe forbids two subscription items on the same subscription from sharing a price, so billing is modeled as **one subscription item per plan (price), with `quantity` equal to the number of the tenant's `active` relays on that plan**. Every such relay's `stripe_subscription_item_id` points at the shared item for its plan; relays that aren't billed (free, inactive, delinquent) have it cleared.

Stripe uses **pay-in-advance** by default: when a subscription is first created, Stripe immediately generates an open invoice for the current period. The `invoice.created` webhook fires shortly after and `handle_invoice_created` attempts payment.

- Fetch the tenant and its relays. Build the desired state: for each `active` relay on a paid plan with a non-empty `stripe_price_id`, count relays per price.
- **Resolve the live subscription**: if the tenant has a `stripe_subscription_id`, fetch it. If Stripe no longer knows about it, or its status is `canceled`/`incomplete_expired`, call `command.clear_tenant_subscription` and treat the tenant as having no subscription.
- **No relays to bill** (desired state empty): if the tenant still has a `stripe_subscription_id`, cancel the Stripe subscription and call `command.clear_tenant_subscription`. Clear `stripe_subscription_item_id` on every relay that has one. Return.
- **No subscription yet**: create a Stripe subscription for the customer with `collection_method: "charge_automatically"` and one item per `(price, quantity)`. Save the subscription ID via `command.set_tenant_subscription`.
- **Existing subscription**: fetch its current items. For each desired `(price, quantity)`: update the matching item's quantity if it differs, otherwise create the item. Delete any item whose price no longer appears in the desired state.
- **Point relays at items**: for each relay, set `stripe_subscription_item_id` (via `command.set_relay_subscription_item`) to the shared item for its plan, or clear it (via `command.delete_relay_subscription_item`) if the relay is not billed.
- **Downgrade validation**: if any quantity decreased or any item was removed, preview the upcoming Stripe invoice and log proration line/amount details to validate expected credit/proration behavior.

## `pub fn handle_webhook(&self, payload: &str, signature: &str) -> Result<()>`

- Verify and parse the event via `self.stripe.construct_event(payload, signature)` (checks the `Stripe-Signature` HMAC and timestamp tolerance — see `spec/stripe.md`)
- Dispatch by type:
  - `invoice.created` -> `self.handle_invoice_created`
  - `invoice.paid` -> `self.handle_invoice_paid`
  - `invoice.payment_failed` -> `self.handle_invoice_payment_failed`
  - `invoice.overdue` -> `self.handle_invoice_overdue`
  - `customer.subscription.updated` -> `self.handle_subscription_updated`
  - `customer.subscription.deleted` -> `self.handle_subscription_deleted`
  - `payment_method.attached` -> `self.handle_payment_method_attached`
- Unknown event types are ignored (return Ok)

## `pub async fn stripe_create_customer(&self, tenant_pubkey: &str) -> Result<String>`

- Resolves a display name via `robot.fetch_nostr_name(tenant_pubkey)`, falling back to the first 8 chars of the pubkey
- Creates the Stripe customer via `stripe.create_customer(display_name, tenant_pubkey)` and returns its id

## `pub async fn stripe_list_invoices(&self, customer_id: &str) -> Result<Value>`

- Delegates to `stripe.list_invoices` — returns the `data` array of the customer's invoices

## `pub async fn stripe_create_portal_session(&self, customer_id: &str, return_url: Option<&str>) -> Result<String>`

- Delegates to `stripe.create_portal_session` — returns the Customer Portal session URL

## `pub async fn get_invoice_with_tenant(&self, invoice_id: &str) -> Result<(Value, Tenant), InvoiceLookupError>`

- Fetches the invoice via `stripe.get_invoice` (a Stripe 4xx surfaces as `InvoiceLookupError::StripeClient`)
- Looks up the tenant by the invoice's `customer` field; errors if the invoice has no customer or no tenant matches

## `pub async fn reconcile_manual_lightning_invoice(&self, invoice_id: &str, invoice: &Value) -> Result<Value, InvoiceLookupError>`

If `invoice.status == "open"` and a manual-Lightning bolt11 was previously issued for it (`query.get_invoice_manual_lightning_bolt11`), check whether that bolt11 has settled (`bitcoin.system_wallet().invoice_settled(...)`). If it has, mark the Stripe invoice paid out of band (`stripe.pay_invoice_out_of_band`) and return the refreshed invoice. On any lookup/settlement failure, log and return the invoice unchanged.

## `pub async fn get_or_create_manual_lightning_bolt11(&self, invoice_id: &str, tenant_pubkey: &str, amount_due_minor: i64, currency: &str) -> Result<String>`

- Returns the existing bolt11 if one is already recorded for the invoice
- Otherwise creates one via `create_bolt11`, records it with `command.insert_manual_lightning_invoice_payment`, and returns it (re-reading the stored row if the insert lost a race)

## `pub async fn create_bolt11(&self, amount_due_minor: i64, currency: &str) -> Result<String>`

- Converts the fiat amount to msats via `bitcoin.fiat_minor_to_msats` (fetches the live BTC spot price — see `spec/bitcoin.md`)
- Issues a bolt11 invoice for that amount on the system NWC wallet (`bitcoin.system_wallet().make_invoice(...)`)
- Returns the bolt11 invoice string

## `pub async fn pay_outstanding_nwc_invoices(&self, tenant: &Tenant) -> Result<()>`

Called when a tenant first sets their NWC URL (via `PUT /tenants/:pubkey`). Attempts to pay any currently open invoices for the tenant using their NWC wallet, so that invoices created before NWC was configured are not left unpaid.

- If `tenant.nwc_url` is empty, return early.
- List all Stripe invoices for `tenant.stripe_customer_id` via `stripe.list_invoices`.
- For each invoice with `status == "open"` and `amount_due > 0`:
  - Attempt NWC payment via `nwc_pay_invoice`.
  - On success: mark the Stripe invoice paid out of band (`stripe.pay_invoice_out_of_band`) and call `command.clear_tenant_nwc_error`.
  - On failure: call `command.set_tenant_nwc_error` and log the error; continue to the next invoice.

## `pub async fn pay_outstanding_card_invoices(&self, tenant: &Tenant) -> Result<()>`

Attempts Stripe-side collection for open invoices when the tenant has a card on file.

- If tenant has no card payment method (`stripe.has_payment_method`), return early.
- List all Stripe invoices for `tenant.stripe_customer_id` via `stripe.list_invoices`.
- For each invoice with `status == "open"` and `amount_due > 0`:
  - Call `stripe.pay_invoice` to retry collection using the card on file.
  - Log and continue on failures.

## `fn handle_invoice_created(&self, invoice: &Invoice)`

Attempts to pay a new subscription invoice. Because Stripe defaults to pay-in-advance, this webhook fires immediately when a subscription is created (i.e. when a paid relay is added or a plan is upgraded). Payment priority:

1. **NWC auto-pay**: If the tenant has a `nwc_url`, run `nwc_pay_invoice` (decrypting the tenant's stored `nwc_url` first):
   - The system wallet (`bitcoin.system_wallet()`) issues a bolt11 invoice for the fiat amount; the tenant's wallet (`Wallet::parse` of the decrypted URL) pays it. A `pending` row in `invoice_nwc_payment` guards against double-charging across retries.
   - If payment succeeds: mark the Stripe invoice paid out of band (`stripe.pay_invoice_out_of_band`) and clear `nwc_error` via `command.clear_tenant_nwc_error`. Done.
   - If it fails before any charge could have gone out: set `nwc_error` on the tenant via `command.set_tenant_nwc_error`, and fall through to the next option (carrying a short summary of the error into the eventual DM).
   - If it fails after a charge may have gone out (needs reconciliation): set `nwc_error` and return the error without falling through — a human must reconcile before any retry.
2. **Card on file**: If the tenant has a payment method on the Stripe customer, do nothing here — Stripe will charge automatically for this invoice attempt.
3. **Manual payment**: If neither NWC nor card is available, send a DM via `robot.send_dm` notifying the tenant that payment is due with a link to the application for manual Lightning payment.

Skip invoices with `amount_due` of 0.

## `fn handle_invoice_paid(&self, invoice: &Invoice)`

- Look up tenant by `stripe_customer_id`
- If tenant has `past_due_at` set:
  - Clear `past_due_at` via `command.clear_tenant_past_due`
  - Find all `delinquent` relays on paid plans for the tenant (relays marked delinquent by the billing system due to non-payment)
  - Reactivate each one via `command.activate_relay`

## `fn handle_invoice_payment_failed(&self, invoice: &Invoice)`

- Look up tenant by `stripe_customer_id`
- If tenant does not already have `past_due_at` set:
  - Set `past_due_at` to now via `command.set_tenant_past_due`
  - Send a DM via `robot.send_dm` notifying the tenant that their payment has failed and their relays may be deactivated if not resolved.

## `fn handle_invoice_overdue(&self, invoice: &Invoice)`

- Look up tenant by `stripe_customer_id`
- Mark all active relays on paid plans as delinquent via `command.mark_relay_delinquent` (sets status to `delinquent`, distinct from user-initiated `deactivate_relay`)
- Send a DM via `robot.send_dm` notifying the tenant that their paid relays have been deactivated due to non-payment

## `fn handle_subscription_updated(&self, subscription: &Subscription)`

- Look up tenant by `stripe_customer_id`
- If subscription status is `canceled` or `unpaid`:
  - Clear `stripe_subscription_id` via `command.clear_tenant_subscription`
  - Mark all active paid relays as delinquent via `command.mark_relay_delinquent`

## `fn handle_subscription_deleted(&self, subscription: &Subscription)`

- Look up tenant by `stripe_customer_id`
- Clear `stripe_subscription_id` via `command.clear_tenant_subscription`

## `fn handle_payment_method_attached(&self, stripe_customer_id: &str)`

- Look up tenant by `stripe_customer_id`
- Call `pay_outstanding_card_invoices` so invoices that were due before card setup are retried immediately
