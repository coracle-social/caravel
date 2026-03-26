# Admin Tenant List

The admin tenant list page shows all tenants with profile-enriched search results.

```pug
page(path="/admin/tenants", auth="required", role="admin", shell="app")
  header
    title Tenants

  controls
    input(type="search", name="query", placeholder="Search tenants...")

  loading_state(message="Loading tenants...")
  error_state(message="Failed to load tenants.")

  list(empty="No tenants found")
    item(link="/admin/tenants/:pubkey")
      avatar(profile.picture)
      title profile.name || shortened_pubkey
      subtitle profile.about || "No profile bio"
      meta tenant.pubkey
      badge tenant
```
