# Relay Detail

The relay detail page shows relay status and settings for an authenticated tenant, with inline toggles and plan changes.

```pug
page(path="/relays/:id", auth="required", shell="app")
  back_link(href="/relays", label="Relays")

  loading_state(message="Loading relay...")
  error_state(message="Failed to load relay.")

  relay_detail_card(edit_href="/relays/:id/edit")
    actions
      button(action="deactivate_relay") Deactivate
    toggles
      toggle(name="policy_public_join") Public join
      toggle(name="policy_strip_signatures") Strip signatures
      toggle(name="groups_enabled") Groups
      toggle(name="management_enabled") Management API
      toggle(name="blossom_enabled") Media storage
      toggle(name="livekit_enabled") LiveKit support
      toggle(name="push_enabled") Push notifications
    plan_selector(action="update_plan")
    member_count(source="relay_url")

  error_message(on="mutation_failure")
```
