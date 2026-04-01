# `pub struct Billing`

Billing encapsulates logic related to synchronizing state with Stripe.

Members:

- `nwc_url: String` - a nostr wallet connect URL used to **create** bolt11 invoices
- `repo: Repo`
- `robot: Robot`

## `pub fn new(repo: Repo, robot: Robot) -> Self`

- Reads environment and populates members
