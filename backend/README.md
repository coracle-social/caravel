# Backend

Rust backend for Caravel. It manages tenants, relays, invoices, and background workers for relay provisioning and billing.

## Tech Stack

- Rust (Edition 2024)
- Axum (HTTP API)
- SQLx + SQLite
- Tokio (async runtime + workers)
- Nostr SDK (NIP-98 auth, NIP-17 DMs, NIP-47 wallet connect, NIP-44 encryption at rest)

## Layout

```
backend/
  migrations/
    0001_init.sql
  spec/            # Module-by-module design notes
  src/
    main.rs        # App bootstrap: load Env, build services, serve + spawn workers
    env.rs         # Configuration from the environment (+ NIP-44 encryption, NIP-98 signing)
    api.rs         # Shared Api state, router, NIP-98 auth + authorization helpers
    web.rs         # HTTP response envelope + helpers
    routes/        # HTTP route handlers (identity, plans, tenants, relays, invoices, stripe)
    models.rs      # Domain models + sqlite rows
    query.rs       # Database reads
    command.rs     # Database writes + activity broadcast
    pool.rs        # SQLite pool + migrations
    billing.rs     # Stripe subscription reconciliation + Lightning collection worker
    stripe.rs      # Thin Stripe REST client
    wallet.rs      # NWC wallet handle (NIP-47)
    bitcoin.rs     # Fiat ↔ BTC/msats conversion
    infra.rs       # Zooid relay-sync worker
    robot.rs       # Nostr robot identity + DM sending
```

## Configuration

All configuration is read from the environment by `Env::load()` at startup. **Every variable below is required**: `Env::load()` panics if any is missing or blank, and comma-separated lists must contain at least one entry. Copy `.env.template` to `.env` to get started.

**Server**

| Variable               | Description                                                          |
| ---------------------- | ------------------------------------------------------------------- |
| `SERVER_HOST`          | API bind host; also the value the NIP-98 `u` tag must contain       |
| `SERVER_PORT`          | API bind port                                                       |
| `SERVER_ADMIN_PUBKEYS` | Comma-separated admin pubkeys (hex)                                 |
| `SERVER_ALLOW_ORIGINS` | Comma-separated allowed CORS origins                                |
| `DATABASE_URL`         | SQLite URL; relative `sqlite://` paths are resolved under `backend/` |

**Robot identity**

| Variable                 | Description                                                                                        |
| ------------------------ | -------------------------------------------------------------------------------------------------- |
| `ROBOT_SECRET`           | Robot Nostr secret key; used for signing, NIP-44 encryption of stored NWC URLs, and NIP-98 auth    |
| `ROBOT_NAME`             | Robot display name (kind `0`)                                                                      |
| `ROBOT_DESCRIPTION`      | Robot description (kind `0`)                                                                        |
| `ROBOT_PICTURE`          | Robot picture URL (kind `0`)                                                                        |
| `ROBOT_WALLET`           | System NWC URL used to issue and look up BOLT11 invoices                                            |
| `ROBOT_OUTBOX_RELAYS`    | Comma-separated relays the robot publishes its profile and kind `10002` relay list to              |
| `ROBOT_INDEXER_RELAYS`   | Comma-separated relays used for recipient relay/profile discovery                                  |
| `ROBOT_MESSAGING_RELAYS` | Comma-separated DM relays published as kind `10050`                                                |

**Relay hosting (zooid / livekit)**

| Variable             | Description                                             |
| -------------------- | ------------------------------------------------------- |
| `ZOOID_API_URL`      | Zooid API base URL used by the infra sync worker        |
| `RELAY_DOMAIN`       | Base domain appended to relay subdomains                |
| `LIVEKIT_URL`        | LiveKit URL sent to zooid when a relay enables livekit   |
| `LIVEKIT_API_KEY`    | LiveKit API key sent to zooid                           |
| `LIVEKIT_API_SECRET` | LiveKit API secret sent to zooid                        |

**Blossom S3** — sent to zooid as the S3 adapter config (with `key_prefix` = relay id) when a relay enables blossom.

| Variable                | Description           |
| ----------------------- | --------------------- |
| `BLOSSOM_S3_ENDPOINT`   | S3 endpoint URL       |
| `BLOSSOM_S3_REGION`     | S3 region             |
| `BLOSSOM_S3_BUCKET`     | S3 bucket name        |
| `BLOSSOM_S3_ACCESS_KEY` | S3 access key ID      |
| `BLOSSOM_S3_SECRET_KEY` | S3 secret access key  |

**Billing (Stripe)**

| Variable                | Description                                                             |
| ----------------------- | ----------------------------------------------------------------------- |
| `STRIPE_SECRET_KEY`     | Stripe API secret key used for billing API operations                   |
| `STRIPE_WEBHOOK_SECRET` | Stripe webhook signing secret used to verify `Stripe-Signature` headers |
| `STRIPE_PRICE_BASIC`    | Stripe price ID (`price_...`) backing the Basic plan                    |
| `STRIPE_PRICE_GROWTH`   | Stripe price ID (`price_...`) backing the Growth plan                   |

Comma-separated list variables are split on commas and trimmed; empty entries are dropped.

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
- `PUT /tenants/:pubkey` — update tenant `nwc_url` (admin or same tenant)
- `GET /tenants/:pubkey/relays` — list tenant relays (admin or same tenant)
- `GET /relays` — list relays (admin)
- `POST /relays` — create relay (admin or relay tenant)
- `GET /relays/:id` — get relay (admin or relay tenant)
- `GET /relays/:id/members` — list relay members from zooid (admin or relay tenant)
- `PUT /relays/:id` — update relay (admin or relay tenant)
- `GET /relays/:id/activity` — list relay activity (admin or relay tenant)
- `POST /relays/:id/deactivate` — deactivate relay (admin or relay tenant)
- `POST /relays/:id/reactivate` — reactivate relay (admin or relay tenant)
- `GET /tenants/:pubkey/invoices` — list tenant invoices (admin or same tenant)
- `GET /invoices/:id` — get invoice (admin or same tenant)
- `GET /invoices/:id/bolt11` — get invoice bolt11 (admin or same tenant)
- `GET /tenants/:pubkey/stripe/session` — create Stripe customer portal session (admin or same tenant)

## API Auth Model

Caravel intentionally uses a session-style variant of NIP-98 for client-to-backend API auth.

- Frontend signs one kind `27235` event with `u = VITE_API_URL` and caches that header for about 10 minutes.
- Backend verifies event kind, signature, and that `u` contains configured `SERVER_HOST`.
- Backend intentionally does not bind auth to exact request URL/method/query, and does not enforce payload hash, timestamp freshness window, or replay cache.
- Goal: reduce repeated wallet signing prompts and avoid cookie-based sessions.
- Tradeoff: this is weaker request-intent binding than strict NIP-98 semantics.
