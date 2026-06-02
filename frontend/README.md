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
| `VITE_API_URL` | Backend API base URL | `http://127.0.0.1:2892` |
| `VITE_RELAY_DOMAIN` | Domain relay subdomains live under (match backend `RELAY_DOMAIN`) | `spaces.coracle.social` |

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

- Tenant requests use an intentional session-style variant of NIP-98:
  - The client signs one kind `27235` event with `u = VITE_API_URL`.
  - The resulting `Authorization` header is cached for about 10 minutes to avoid repeated signer prompts.
  - The backend validates signer identity + host affinity rather than exact URL/method binding per request.
- Admin routes require a pubkey listed in `ADMINS` on the backend.

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
