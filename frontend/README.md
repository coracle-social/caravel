# Frontend

SolidJS frontend for the Caravel platform. This app includes the marketing site, tenant dashboard, and super admin dashboard, and talks to the backend API using NIP-98 authentication.

## Tech Stack

- TypeScript
- SolidJS + Vite
- Tailwind CSS + Preline UI
- applesauce (Nostr tooling)
- nonboard (Nostr login)
- TanStack Query (data fetching)

## Layout

```
frontend/
  public/
  src/
    assets/
    components/       # Shared UI components
    lib/              # Nostr helpers
    pages/            # Route components
    App.tsx           # Router + layout
    index.tsx         # App entrypoint
    index.css         # Global styles
```

## Configuration

Environment variables (see `.env.template`):

| Variable | Description | Default |
|---|---|---|
| `VITE_API_URL` | Backend API base URL | `http://127.0.0.1:3000` |

## Running

```bash
cd frontend
npm install
npm run dev
```

Build and preview:

```bash
npm run build
npm run preview
```

## Authentication

- Tenant requests use NIP-98 tokens derived from the logged-in user
- Admin routes require a pubkey listed in `PLATFORM_ADMIN_PUBKEYS` on the backend

## Routes

Marketing and onboarding:

- `/` — marketing page with a “Get Started” CTA
- `/login` — login via nonboard

Tenant dashboard:

- `/relays` — list relays
- `/relays/new` — create relay + invoice flow
- `/relays/:id` — relay detail
- `/relays/:id/edit` — edit relay
- `/account` — billing + invoice history

Super admin dashboard:

- `/admin/tenants` — list tenants
- `/admin/tenants/:id` — tenant detail
- `/admin/relays` — list relays
- `/admin/relays/:id` — relay detail
- `/admin/relays/:id/edit` — edit relay

## Todos

- [ ] Marketing page (`/`) with value props, features, and CTA
- [ ] Tenant dashboard auth via NIP-98
  - [ ] Relays list (`/relays`) with search/filter and add relay CTA
  - [ ] Relay detail (`/relays/:id`) with edit + deactivate actions
  - [ ] New relay form (`/relays/new`) with plan selection + invoice flow
  - [ ] Relay edit form (`/relays/:id/edit`)
  - [ ] Account page (`/account`) with status, invoices, and recurring billing toggle
- [ ] Super admin dashboard auth via `PLATFORM_ADMIN_PUBKEYS`
  - [ ] Tenants list (`/admin/tenants`)
  - [ ] Tenant detail (`/admin/tenants/:id`) with status + deactivate actions
  - [ ] Relays list (`/admin/relays`)
  - [ ] Relay detail (`/admin/relays/:id`)
  - [ ] Relay edit form (`/admin/relays/:id/edit`)
