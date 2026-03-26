# Relay Edit

The relay edit page lets an authenticated tenant update relay metadata and returns to relay detail on success.

```pug
page(path="/relays/:id/edit", auth="required", shell="app")
  back_link(href="/relays/:id", label="Back")
  header
    title Edit Relay

  loading_state(message="Loading relay...")
  error_state(message="Failed to load relay.")

  form(action="update_tenant_relay")
    field(name="name", label="Relay Name", required=true)
    field(name="subdomain", label="Subdomain", required=true)
    field(name="icon", label="Icon URL", type="url")
    field(name="description", label="Description", type="textarea")
    submit_button(default="Save Changes", loading="Saving...")
    error_message(on="submit_failure")
```
