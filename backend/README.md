# Backend

Rust backend for Caravel. It manages tenants, relays, invoices, and background workers for relay provisioning and billing.

## Tech Stack

- Rust (Edition 2024)
- Axum (HTTP API)
- SQLx + SQLite
- Tokio (async runtime + workers)
- Nostr SDK (NIP-98 auth, NIP-17 DMs, NIP-47 wallet connect)

## Layout

```
backend/
  migrations/
    0001_init.sql
  src/
    api.rs       # Axum routes + NIP-98 auth checks
    billing.rs   # Invoice generation + collection worker
    infra.rs     # Zooid sync worker
    main.rs      # App bootstrap
    models.rs    # DB models
    repo.rs      # Data access layer
    robot.rs     # Nostr robot identity + DM sending
```

## Configuration

Environment variables:

| Variable                 | Description                                                             | Default                              |
| ------------------------ | ----------------------------------------------------------------------- | ------------------------------------ |
| `DATABASE_URL`           | SQLite URL. Relative paths are resolved under `backend/`.               | `sqlite://<backend>/data/caravel.db` |
| `HOST`                   | API bind host (also used for NIP-98 `u` host check)                     | `127.0.0.1`                          |
| `PORT`                   | API bind port                                                           | `2892`                               |
| `ADMINS`                 | Comma-separated admin pubkeys (hex)                                     | _optional_                           |
| `ALLOW_ORIGINS`          | Comma-separated CORS origins. If empty, CORS is permissive.             | _optional_                           |
| `ZOOID_API_URL`          | Zooid API base URL used by infra worker                                 | _required for infra sync_            |
| `ZOOID_API_SECRET`       | Nostr secret key used for authentication of requests to the zooid API   | _required_                           |
| `RELAY_DOMAIN`           | Base domain appended to relay subdomains                                | empty                                |
| `LIVEKIT_URL`            | LiveKit URL sent to zooid when relay livekit is enabled                 | _optional_                           |
| `LIVEKIT_API_KEY`        | LiveKit API key sent to zooid                                           | _optional_                           |
| `LIVEKIT_API_SECRET`     | LiveKit API secret sent to zooid                                        | _optional_                           |
| `NWC_URL`                | Platform NWC URL used to generate BOLT11 invoices                       | _required for invoice generation_    |
| `STRIPE_SECRET_KEY`      | Stripe API secret key used for billing API operations                   | _required_                           |
| `STRIPE_WEBHOOK_SECRET`  | Stripe webhook signing secret used to verify `Stripe-Signature` headers | _required_                           |
| `ROBOT_SECRET`           | Robot Nostr secret key                                                  | _required_                           |
| `ROBOT_NAME`             | Robot display name (kind `0`)                                           | _optional_                           |
| `ROBOT_DESCRIPTION`      | Robot description (kind `0`)                                            | _optional_                           |
| `ROBOT_PICTURE`          | Robot picture URL (kind `0`)                                            | _optional_                           |
| `ROBOT_OUTBOX_RELAYS`    | Comma-separated relays published as kind `10002`                        | _required_                           |
| `ROBOT_INDEXER_RELAYS`   | Comma-separated relays used for recipient relay discovery               | _required_                           |
| `ROBOT_MESSAGING_RELAYS` | Comma-separated relays published as kind `10050`                        | _required_                           |

Relay list env vars are comma-separated and trimmed. If a relay has no `ws://` or `wss://` scheme, `wss://` is prepended.

## Schema and Architecture

See [spec](spec) for more details

## API Routes

Most API routes are NIP-98 protected.

Public exceptions:

- `GET /plans`
- `GET /plans/:id`
- `POST /stripe/webhook` (validated with Stripe signatures)

- `GET /identity` — get auth identity (`pubkey`, `is_admin`); side-effect-free
- `GET /tenants` — list tenants (admin)
- `POST /tenants` — idempotently ensure a tenant row exists for the current auth pubkey (creates Stripe customer + tenant on first call, returns existing tenant otherwise)
- `GET /tenants/:pubkey` — get tenant (admin or same tenant)
- `PUT /tenants/:pubkey/billing` — update tenant `nwc_url` (admin or same tenant)
- `GET /relays` — list relays (`?tenant=<pubkey>` allowed for admin only)
- `POST /relays` — create relay (admin or relay tenant)
- `GET /relays/:id` — get relay (admin or relay tenant)
- `PUT /relays/:id` — update relay (admin or relay tenant)
- `POST /relays/:id/deactivate` — deactivate relay (admin or relay tenant)
- `GET /invoices` — list invoices (`?tenant=<pubkey>` allowed for admin only)

## API Auth Model

Caravel intentionally uses a session-style variant of NIP-98 for client-to-backend API auth.

- Frontend signs one kind `27235` event with `u = VITE_API_URL` and caches that header for about 10 minutes.
- Backend verifies event kind, signature, and that `u` contains configured `HOST`.
- Backend intentionally does not bind auth to exact request URL/method/query, and does not enforce payload hash, timestamp freshness window, or replay cache.
- Goal: reduce repeated wallet signing prompts and avoid cookie-based sessions.
- Tradeoff: this is weaker request-intent binding than strict NIP-98 semantics.
