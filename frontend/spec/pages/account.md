# Account

The account page lets an authenticated tenant manage billing settings and review invoice history.

```pug
page(path="/account", auth="required", shell="app")
  header
    title My Account
    button Log out

  section(id="status")
    heading Account Status
    badge tenant

  section(id="billing")
    heading Recurring Billing
    description Enable automatic payments by providing your Nostr Wallet Connect URL.
    input(name="nwc_url", placeholder="nostr+walletconnect://...")
    button Save
    error_message(on="save_failure")

  section(id="invoices")
    heading Invoice History
    loading_state(message="Loading invoices...")
    list(empty="No invoices yet")
      item
        status
        created_at
        bolt11
```
