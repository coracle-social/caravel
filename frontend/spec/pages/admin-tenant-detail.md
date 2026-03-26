# Admin Tenant Detail

The admin tenant detail page shows tenant status and all relays for a selected tenant.

```pug
page(path="/admin/tenants/:id", auth="required", role="admin", shell="app")
  back_link(href="/admin/tenants", label="Tenants")
  header
    title Tenant :id

  loading_state(message="Loading tenant...")
  error_state(message="Failed to load tenant.")

  section(id="status")
    heading Status
    text Current: tenant

  section(id="relays")
    heading Relays
    list(empty="No relays")
      item(link="/admin/relays/:id")
        title relay.info_name || relay.subdomain
        subtitle {relay.subdomain}.spaces.coracle.social
        status relay.status
```
