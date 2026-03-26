# Relay New

The new relay page lets an authenticated tenant configure and create a relay, then navigates to its detail page.

```pug
page(path="/relays/new", auth="required", shell="app")
  header
    title New Relay

  form(action="create_tenant_relay")
    field(name="name", label="Relay Name", required=true, placeholder="My Community")
    field(name="subdomain", label="Subdomain", required=true, suffix=".spaces.coracle.social", placeholder="my-community")
    field(name="icon", label="Icon URL", type="url", placeholder="https://example.com/icon.png")
    field(name="description", label="Description", type="textarea", placeholder="A community for...")

    field(name="plan", label="Plan")
      option(value="free")
        title Free
        price Free
        members Up to 10 members
      option(value="basic")
        title Basic
        price 10,000 sats/mo
        members Up to 100 members
      option(value="growth")
        title Growth
        price 50,000 sats/mo
        members Unlimited members

    tooltip_error(field="subdomain")
    submit_button(default="Create Relay", loading="Creating...")
```
