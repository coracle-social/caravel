# Admin Relay Detail

The admin relay detail page exposes relay state and controls with admin-level editing and deactivation.

```pug
page(path="/admin/relays/:id", auth="required", role="admin", shell="app")
  back_link(href="/admin/relays", label="Relays")

  loading_state(message="Loading relay...")
  error_state(message="Failed to load relay.")

  relay_detail_card(edit_href="/admin/relays/:id/edit", show_tenant=true, enforce_plan_limits=false, show_plan_actions=false)
    actions
      button(action="admin_deactivate_relay") Deactivate
    toggles
      toggle(name="policy_public_join") Public join
      toggle(name="policy_strip_signatures") Strip signatures
      toggle(name="groups_enabled") Groups
      toggle(name="management_enabled") Management API
      toggle(name="blossom_enabled") Media storage
      toggle(name="livekit_enabled") LiveKit support
      toggle(name="push_enabled") Push notifications

  error_message(on="mutation_failure")
```
