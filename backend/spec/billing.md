# `pub struct Billing`

Billing encapsulates logic related to synchronizing state with Stripe, processing payments via NWC, and managing subscription lifecycle.

Members:

- `nwc_url: String` - a nostr wallet connect URL used to **create** bolt11 invoices (i.e. receive payments), from `NWC_URL`
- `stripe_secret_key: String` - Stripe API key used for billing API operations, from `STRIPE_SECRET_KEY`
- `stripe_webhook_secret: String` - secret for verifying Stripe webhook signatures, from `STRIPE_WEBHOOK_SECRET`
- `query: Query`
- `command: Command`
- `robot: Robot`

## `pub fn new(query: Query, command: Command, robot: Robot) -> Self`

- Reads environment and populates members
- Panics if `STRIPE_SECRET_KEY` is missing/empty
- Panics if `STRIPE_WEBHOOK_SECRET` is missing/empty

## `pub fn start(&self)`

- Subscribes to `command.notify.subscribe()`
- On `create_relay`, `update_relay`, `activate_relay`, `deactivate_relay`, `fail_relay_sync`, and `complete_relay_sync`, call `self.sync_relay_subscription`.

## `pub fn sync_relay_subscription(&self, activity: &Activity)`

Resolves the relay associated with `activity` and reconciles that relay's tenant via `sync_tenant_subscription`. The startup/lagged reconcile loop calls `sync_tenant_subscription` for every tenant.

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

- Verify the webhook signature using `self.stripe_webhook_secret`
- Parse the event and dispatch by type:
  - `invoice.created` -> `self.handle_invoice_created`
  - `invoice.paid` -> `self.handle_invoice_paid`
  - `invoice.payment_failed` -> `self.handle_invoice_payment_failed`
  - `invoice.overdue` -> `self.handle_invoice_overdue`
  - `customer.subscription.updated` -> `self.handle_subscription_updated`
  - `customer.subscription.deleted` -> `self.handle_subscription_deleted`
  - `payment_method.attached` -> `self.handle_payment_method_attached`
- Unknown event types are ignored (return Ok)

## `pub async fn stripe_list_invoices(&self, customer_id: &str) -> Result<Value>`

- Fetches invoices from Stripe API for the given customer
- Returns the `data` array from the Stripe response

## `pub async fn stripe_get_invoice(&self, invoice_id: &str) -> Result<Value>`

- Fetches a single invoice from Stripe API by ID
- Returns the full Stripe invoice object

## `pub async fn create_bolt11(&self, amount_due_cents: i64) -> Result<String>`

- Creates a bolt11 Lightning invoice for the given amount using the system NWC wallet (`self.nwc_url`)
- Returns the bolt11 invoice string

## `pub async fn stripe_create_portal_session(&self, customer_id: &str) -> Result<String>`

- Creates a Stripe Customer Portal session for the given customer
- Returns the portal session URL

## `pub async fn pay_outstanding_nwc_invoices(&self, tenant: &Tenant) -> Result<()>`

Called when a tenant first sets their NWC URL (via `PUT /tenants/:pubkey`). Attempts to pay any currently open invoices for the tenant using their NWC wallet, so that invoices created before NWC was configured are not left unpaid.

- If `tenant.nwc_url` is empty, return early.
- List all Stripe invoices for `tenant.stripe_customer_id` via `stripe_list_invoices`.
- For each invoice with `status == "open"` and `amount_due > 0`:
  - Attempt NWC payment via `nwc_pay_invoice`.
  - On success: call `stripe_pay_invoice_out_of_band` and `command.clear_tenant_nwc_error`.
  - On failure: call `command.set_tenant_nwc_error` and log the error; continue to the next invoice.

## `pub async fn pay_outstanding_card_invoices(&self, tenant: &Tenant) -> Result<()>`

Attempts Stripe-side collection for open invoices when the tenant has a card on file.

- If tenant has no card payment method, return early.
- List all Stripe invoices for `tenant.stripe_customer_id`.
- For each invoice with `status == "open"` and `amount_due > 0`:
  - Call Stripe `POST /v1/invoices/:id/pay` to retry collection using the card on file.
  - Log and continue on failures.

## `fn handle_invoice_created(&self, invoice: &Invoice)`

Attempts to pay a new subscription invoice. Because Stripe defaults to pay-in-advance, this webhook fires immediately when a subscription is created (i.e. when a paid relay is added or a plan is upgraded). Payment priority:

1. **NWC auto-pay**: If the tenant has a `nwc_url`:
   - Create a bolt11 Lightning invoice for the invoice amount using `self.nwc_url` (the receiving/system wallet)
   - Pay the bolt11 invoice using the tenant's `nwc_url` (the spending/tenant wallet)
   - If payment succeeds: call Stripe `POST /v1/invoices/:id/pay` with `paid_out_of_band: true`. Clear `nwc_error` via `command.clear_tenant_nwc_error`.
   - If payment fails: set `nwc_error` on tenant via `command.set_tenant_nwc_error`. Fall through to next option.
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
