# `pub struct Billing`

Billing encapsulates logic related to synchronizing state with Stripe, processing payments via NWC, and managing subscription lifecycle.

Members:

- `nwc_url: String` - a nostr wallet connect URL used to **create** bolt11 invoices (i.e. receive payments), from `NWC_URL`
- `stripe_webhook_secret: String` - secret for verifying Stripe webhook signatures, from `STRIPE_WEBHOOK_SECRET`
- `query: Query`
- `command: Command`
- `robot: Robot`

## `pub fn new(query: Query, command: Command, robot: Robot) -> Self`

- Reads environment and populates members

## `pub fn start(&self)`

- Subscribes to `command.notify.subscribe()`
- On `create_relay`, `update_relay`, `activate_relay`, `deactivate_relay`, `fail_relay_sync`, and `complete_relay_sync`, call `self.sync_relay_subscription`.

## `pub fn sync_relay_subscription(&self, activity: &Activity)`

Manages the Stripe subscription and subscription items for a relay's tenant. Only paid (non-free) relays interact with Stripe. Free-only tenants have no subscription. Must be idempotent.

- Fetch the relay and tenant associated with the `activity`
- **If relay plan is `free`**: if the relay has a `stripe_subscription_item_id`, delete it via the Stripe API and call `command.delete_relay_subscription_item`. Then check cleanup (below). Return early.
- **If relay is `inactive`**: if the relay has a `stripe_subscription_item_id`, delete it via the Stripe API and call `command.delete_relay_subscription_item`. Then check cleanup (below). Return early.
- **If relay is `active` and on a paid plan**:
  - **Ensure subscription exists**: If the tenant has no `stripe_subscription_id`, create a Stripe subscription for the customer with `collection_method: "charge_automatically"` and the relay's price as the first item. Save the subscription ID via `command.set_tenant_subscription` and the item ID via `command.set_relay_subscription_item`. Return early.
  - **Sync the subscription item**: If the tenant already has a subscription, create or update the relay's Stripe subscription item to the plan's `stripe_price_id` via the Stripe API, then call `command.set_relay_subscription_item`.
- **Clean up empty subscription**: After any item deletion, check if the tenant has any remaining active paid relays. If none and the tenant has a `stripe_subscription_id`, cancel the Stripe subscription immediately and call `command.clear_tenant_subscription`.

## `pub fn handle_webhook(&self, payload: &str, signature: &str) -> Result<()>`

- Verify the webhook signature using `self.stripe_webhook_secret`
- Parse the event and dispatch by type:
  - `invoice.created` -> `self.handle_invoice_created`
  - `invoice.paid` -> `self.handle_invoice_paid`
  - `invoice.payment_failed` -> `self.handle_invoice_payment_failed`
  - `invoice.overdue` -> `self.handle_invoice_overdue`
  - `customer.subscription.updated` -> `self.handle_subscription_updated`
  - `customer.subscription.deleted` -> `self.handle_subscription_deleted`
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

## `fn handle_invoice_created(&self, invoice: &Invoice)`

Attempts to pay a new subscription invoice. Payment priority:

1. **NWC auto-pay**: If the tenant has a `nwc_url`:
   - Create a bolt11 Lightning invoice for the invoice amount using `self.nwc_url` (the receiving/system wallet)
   - Pay the bolt11 invoice using the tenant's `nwc_url` (the spending/tenant wallet)
   - If payment succeeds: call Stripe `POST /v1/invoices/:id/pay` with `paid_out_of_band: true`. Clear `nwc_error` via `command.clear_tenant_nwc_error`.
   - If payment fails: set `nwc_error` on tenant via `command.set_tenant_nwc_error`. Fall through to next option.
2. **Card on file**: If the tenant has a payment method on the Stripe customer, do nothing — Stripe will charge automatically.
3. **Manual payment**: If neither NWC nor card is available, send a DM via `robot.send_dm` notifying the tenant that payment is due with a link to the application for manual Lightning payment.

Skip invoices with `amount_due` of 0.

## `fn handle_invoice_paid(&self, invoice: &Invoice)`

- Look up tenant by `stripe_customer_id`
- If tenant has `past_due_at` set:
  - Clear `past_due_at` via `command.clear_tenant_past_due`
  - Find all `inactive` relays for the tenant that were deactivated due to non-payment (i.e. relays on paid plans that are inactive)
  - Reactivate each one via `command.activate_relay`

## `fn handle_invoice_payment_failed(&self, invoice: &Invoice)`

- Look up tenant by `stripe_customer_id`
- If tenant does not already have `past_due_at` set:
  - Set `past_due_at` to now via `command.set_tenant_past_due`
  - Send a DM via `robot.send_dm` notifying the tenant that their payment has failed and their relays may be deactivated if not resolved.

## `fn handle_invoice_overdue(&self, invoice: &Invoice)`

- Look up tenant by `stripe_customer_id`
- Deactivate all active relays on paid plans via `command.deactivate_relay`
- Send a DM via `robot.send_dm` notifying the tenant that their paid relays have been deactivated due to non-payment

## `fn handle_subscription_updated(&self, subscription: &Subscription)`

- Look up tenant by `stripe_customer_id`
- If subscription status is `canceled` or `unpaid`:
  - Clear `stripe_subscription_id` via `command.clear_tenant_subscription`
  - Deactivate all active paid relays for the tenant via `command.deactivate_relay`

## `fn handle_subscription_deleted(&self, subscription: &Subscription)`

- Look up tenant by `stripe_customer_id`
- Clear `stripe_subscription_id` via `command.clear_tenant_subscription`

