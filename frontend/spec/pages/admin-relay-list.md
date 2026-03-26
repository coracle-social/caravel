# Admin Relay List

The admin relay list page shows all relays with search for operators.

```pug
page(path="/admin/relays", auth="required", role="admin", shell="app")
  header
    title Relays

  controls
    input(type="search", name="query", placeholder="Search relays...")

  loading_state(message="Loading relays...")
  error_state(message="Failed to load relays.")

  list(empty="No relays found")
    item(link="/admin/relays/:id")
      title relay.info_name || relay.subdomain
      subtitle {relay.subdomain}.spaces.coracle.social
      tenant relay.tenant
      status relay.status
```
