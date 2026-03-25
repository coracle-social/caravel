# `pub struct Billing`

Billing is a service which polls the database, creates invoices, and attempts to collect payment for invoices.

Members:

- `nwc_url: String` - a nostr wallet connect URL used to **create** invoices
- `repo: Repo`
- `robot: Robot`

## `pub fn new(repo: Repo, robot: Robot) -> Self`

- Reads environment and populates members

## `pub async fn start(self)`

Calls `self.tick` in a loop every hour.

## `pub async fn tick(self)`

Iterates over `repo.list_activity` since last run and does the following:

- For any `relay_created|relay_updated|relay_activated` activity if this is the first non-free relay for the tenant, update tenant's billing anchor to the time the relay was created.

Also iterates over `repo.list_tenants()` and for each tenant calls `self.generate_invoice_if_due(tenant)` and `self.collect_outstanding(tenant)`.

## `async fn generate_invoice_if_due(&self, tenant: &Tenant)`

- Skip tenants that have a `pending` invoice or have no active non-free relays.
- Compute current billing period from `tenant.billing_anchor` as rolling monthly windows: `[period_start, period_end)`.
- Only generate an invoice once the period has closed (`now >= period_end`).
- Load activity needed to compute usage for the tenant using `repo.list_activity`.
- Calculate how many hours (rounded up) each relay was active during the window per paid plan.
- If total invoice amount is 0, return.
- Generate a `bolt11` invoice. If this fails, log the error and return.
- Generate an invoice for the tenant and an invoice item for each relay.
- Persist invoice + items atomically using `repo.create_invoice`.

## `async fn collect_outstanding(&self, tenant: &Tenant)`

- Load `pending` tenant invoices and attempt to collect each one.
- If `attempted_at` is less than 24 hours ago, skip it.
- If the `bolt11` invoice has been paid out of band, call `repo.mark_invoice_paid` and return.
- If the tenant has a `nwc_url`, attempt to pay the invoice with nwc.
- If collection succeeds, call `repo.mark_invoice_paid`.
- If collection fails, populate `repo.mark_invoice_attempted`.
- If nwc isn't set up or fails and `sent_at` is not set:
  - Send a NIP 17 DM to the user with the invoice included.
  - Call `repo.mark_invoice_sent`.
- If the invoice is 7 days past `created_at`, call `repo.mark_invoice_closed`.
