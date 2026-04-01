# `pub struct Billing`

Billing encapsulates logic related to synchronizing state with Stripe.

Members:

- `nwc_url: String` - a nostr wallet connect URL used to **create** bolt11 invoices
- `query: Query`
- `command: Command`
- `robot: Robot`

## `pub fn new(query: Query, command: Command, robot: Robot) -> Self`

- Reads environment and populates members

## `pub fn start(&self)`

- Subscribes to `command.notify.notified`
  - On `create_relay`, `update_relay`, `activate_relay`, `deactivate_relay`, `fail_relay_sync`, and `complete_relay_sync`, call `self.sync_relay_subscription_item`.

## `pub fn sync_relay_subscription_item(&self, activity: &Activity)`

- Fetch the relay associated with the `activity`
- If the relay has `sync_error`, `synced` is false, `plan` is `free`, or `status` is `inactive`, delete the relay's subscription item using the Stripe api, and clear it with `command.delete_relay_subscription_item`.
- Otherwise, create/update the relay's subscription item to the appropriate Stripe price using the Stripe api and set it with `command.set_relay_subscription_item`.
- This method should be idempotent
