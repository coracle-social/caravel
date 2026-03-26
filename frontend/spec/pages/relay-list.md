# Relay List

The relay list page shows an authenticated tenant's relays with search and status filtering.

```pug
page(path="/relays", auth="required", shell="app")
  header
    title My Relays
    button(href="/relays/new") Add Relay

  controls
    input(type="search", name="query", placeholder="Search by name or subdomain")
    select(name="status")
      option(value="all") All statuses
      option(value="active") Active
      option(value="pending") Pending
      option(value="deactivated") Deactivated
      option(value="provisioning_failed") Provisioning failed
      option(value="suspended") Suspended

  loading_state(message="Loading relays...")
  error_state(message="Failed to load relays.")

  list(empty="No relays found")
    item(link="/relays/:id")
      title relay.info_name || relay.subdomain
      subtitle https://{relay.subdomain}.spaces.coracle.social
      status relay.status
```
