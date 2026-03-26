# Admin Relay Edit

The admin relay edit page updates relay metadata and returns to the admin relay detail view.

```pug
page(path="/admin/relays/:id/edit", auth="required", role="admin", shell="app")
  back_link(href="/admin/relays/:id", label="Back")
  header
    title Edit Relay (Admin)

  loading_state(message="Loading relay...")
  error_state(message="Failed to load relay.")

  form(action="admin_update_relay")
    field(name="name", label="Relay Name", required=true)
    field(name="subdomain", label="Subdomain", required=true)
    field(name="icon", label="Icon URL", type="url")
    field(name="description", label="Description", type="textarea")
    submit_button(default="Save Changes", loading="Saving...")
    error_message(on="submit_failure")
```
