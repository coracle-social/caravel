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
    config.rs          # Env-based configuration
    db.rs              # SQLite pool + migrations
    main.rs            # Axum server entrypoint
    models.rs          # Data models
    repo.rs            # Data access layer
```

## Configuration

Environment variables:

| Variable | Description | Default |
|---|---|---|
| `DATABASE_URL` | SQLite database URL | `sqlite://data/hosting.db` |
| `HOST` | Bind host | `127.0.0.1` |
| `PORT` | Bind port | `3000` |

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

## API Routes

Tenant routes (all require NIP-98 auth; pubkey is inferred from the token):

- `GET /tenant` — fetch (or create) tenant
- `GET /tenant/relays` — list tenant relays
- `POST /tenant/relays` — create relay
- `GET /tenant/relays/:id` — get relay
- `PUT /tenant/relays/:id` — update relay
- `DELETE /tenant/relays/:id` — deactivate relay
- `GET /tenant/invoices` — list invoices

Admin routes (all require NIP-98 auth; pubkey must be in `HOSTING_ADMIN_PUBKEYS`):

- `GET /admin/tenants` — list tenants
- `GET /admin/tenants/:pubkey` — tenant detail (includes relays)
- `PUT /admin/tenants/:pubkey` — update tenant status
- `GET /admin/relays` — list relays
- `GET /admin/relays/:id` — get relay
- `PUT /admin/relays/:id` — update relay
- `DELETE /admin/relays/:id` — deactivate relay

## Next Steps

- Add relay provisioning worker (zooid API calls)
- Add invoice generation and billing jobs
