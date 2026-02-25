# Backend

Rust backend for the Nostr relay hosting platform. This service manages tenants, relays, invoices, and billing, and provisions relays by calling the zooid HTTP API. It authenticates requests using NIP-98.

## Tech Stack

- Rust (Edition 2024)
- Axum (HTTP server)
- SQLx + SQLite (persistence)
- Nostr SDK (NIP-98 verification)

## Layout

```
backend/
  migrations/          # SQL migrations
  src/
    auth.rs            # NIP-98 verification helper
    billing.rs         # Billing loop + invoice generation
    config.rs          # Env-based configuration
    db.rs              # SQLite pool + migrations
    main.rs            # Axum server entrypoint
    models.rs          # Data models
    notifications.rs   # NIP-17 DM sender + relay discovery
    platform.rs        # Startup kind 0/10050 publishing
    provisioning.rs    # Zooid provisioning worker
    repo.rs            # Data access layer
```

## Configuration

Environment variables:

| Variable | Description | Default |
|---|---|---|
| `DATABASE_URL` | SQLite database URL | `sqlite://data/hosting.db` |
| `HOST` | Bind host | `127.0.0.1` |
| `PORT` | Bind port | `3000` |
| `ZOOID_API_URL` | Zooid API base URL | `http://127.0.0.1:8032` |
| `PLATFORM_SECRET` | Platform Nostr secret key for NIP-98 auth | _required_ |
| `RELAY_DOMAIN` | Relay base domain for subdomains | `spaces.coracle.social` |
| `NWC_URL` | Platform NWC URL for invoice generation | _required for billing_ |
| `NOSTR_INDEXER_RELAYS` | Comma-separated relays to fetch kind `10050` DM relays | _required for notifications_ |
| `PLATFORM_NAME` | Platform display name for kind `0` metadata | _optional_ |
| `PLATFORM_DESCRIPTION` | Platform description for kind `0` metadata | _optional_ |
| `PLATFORM_PICTURE` | Platform picture URL for kind `0` metadata | _optional_ |
| `PLATFORM_MESSAGING_RELAYS` | Comma-separated relays published in kind `10050` | _optional_ |

The database directory is created automatically if it doesn’t exist.

## Database Schema

Created via `migrations/0001_init.sql`:

- `tenants`
- `relays`
- `invoices`
- `invoice_items`

## Running

```bash
cd backend
cargo run
```

Health check:

```
GET /healthz
```

## NIP-98 Authentication

NIP-98 verification is implemented in `auth.rs` using the Rust Nostr SDK. It verifies:

- Authorization header format
- Event signature and kind
- URL + HTTP method tags
- Timestamp validity

This is ready to be used by API routes.

## Billing Jobs

The backend runs an in-process billing loop that:

- Generates monthly invoices (using `NWC_URL`)
- Uses the tenant’s `tenant_nwc_url` for recurring pull payments (if set)
- Sends NIP-17 DMs with invoices when recurring is off
- Sends NIP-17 DMs on successful payment when recurring is on

NIP-17 relay discovery:

- Uses `NOSTR_INDEXER_RELAYS` to fetch kind `10050` for each tenant
- Cached for a short period
- If no relays are found, no DM is sent

On startup, the backend publishes:

- Kind `0` metadata (name/description)
- Kind `10050` relay list for DMs

These are published to the relays listed in `NOSTR_INDEXER_RELAYS`.

## API Routes

Tenant routes (all require NIP-98 auth; pubkey is inferred from the token):

- `GET /tenant` — fetch (or create) tenant
- `GET /tenant/relays` — list tenant relays
- `POST /tenant/relays` — create relay
- `GET /tenant/relays/:id` — get relay
- `PUT /tenant/relays/:id` — update relay
- `DELETE /tenant/relays/:id` — deactivate relay
- `GET /tenant/invoices` — list invoices
- `PUT /tenant/billing` — update tenant billing (NWC URL)

Admin routes (all require NIP-98 auth; pubkey must be in `HOSTING_ADMIN_PUBKEYS`):

- `GET /admin/tenants` — list tenants
- `GET /admin/tenants/:pubkey` — tenant detail (includes relays)
- `PUT /admin/tenants/:pubkey` — update tenant status
- `GET /admin/relays` — list relays
- `GET /admin/relays/:id` — get relay
- `PUT /admin/relays/:id` — update relay
- `DELETE /admin/relays/:id` — deactivate relay
