# `pub struct Billing`

Billing encapsulates logic related to synchronizing state with Stripe.

Members:

- `nwc_url: String` - a nostr wallet connect URL used to **create** bolt11 invoices
- `query: Query`
- `command: Command`
- `robot: Robot`

## `pub fn new(query: Query, command: Command, robot: Robot) -> Self`

- Reads environment and populates members
- Subscribes to `command.notify.notified`
