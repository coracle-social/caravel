The frontend is a marketing site combined with a tenant and admin dashboard, both of which depend on the backend's api. Tenants are able to provision new relays, manage existing relays, and deactivate their relays. They can also manage billing information and service level. Admins are able to do this as well, but have an additional admin dashboard where they can view and manage all tenants and relays.

- See ./lib.md for a list of utilities
- See ./pages.md for descriptions of each page
- See ./components.md for a list of reusable components

The frontend implementation's file structure and contents should match the spec.

Dependencies:

- `applesauce` for all nostr-related functionality
