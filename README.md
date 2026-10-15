# Caravel

A multi-tenant platform for hosting Nostr community relays. Tenants create relays, choose their plans and settings, and pay for them; the platform bills them monthly and keeps each relay's configuration on [zooid](https://github.com/coracle-social/zooid), which serves the relays.

## Layout

The backend is a Rust workspace whose crates are layered, each depending only on those listed before it:

- `crates/db` — the Postgres pool, migrations and transactions
- `crates/adapters` — clients for Stripe, Nostr Wallet Connect, the robot's Nostr identity, DNS and zooid
- `crates/core` — the domain: tenants, relays, billing, usage, custom domains, sessions
- `crates/api` — the JSON API, documented in [crates/api/README.md](./crates/api/README.md)
- `crates/app` — the `caravel` binary, which reads the environment, serves the API and runs the background workers

The dashboard is a SolidJS app in `frontend/`.

## Configuration

The binary reads these from the environment, or from a `.env` file in its working directory. [`.env.template`](./.env.template) has local defaults.

| Variable | Description |
| --- | --- |
| `PORT` | The port the API listens on, `2892` by default |
| `API_URL` | The API's public base url, which NIP-98 signatures are checked against |
| `ADMIN_PUBKEYS` | Comma-separated hex pubkeys of the platform's admins |
| `DATABASE_URL` | A Postgres url |
| `ROBOT_SECRET` | The robot's hex Nostr secret key. It signs the robot's DMs and zooid requests, and encrypts stored secrets |
| `ROBOT_NAME`, `ROBOT_DESCRIPTION`, `ROBOT_PICTURE` | The robot's profile, all optional |
| `ROBOT_WALLET` | The platform's NWC url, which issues the Lightning invoices tenants pay |
| `ROBOT_OUTBOX_RELAYS` | Relays the robot publishes its profile and relay list to |
| `ROBOT_INDEXER_RELAYS` | Relays the robot looks up recipients' relay lists and profiles on |
| `ROBOT_MESSAGING_RELAYS` | Relays the robot receives DMs on |
| `RELAY_DOMAINS` | Comma-separated domains relay subdomains live under |
| `ZOOID_API_URL` | Zooid's API, whose `API_WHITELIST` must include the robot's pubkey |
| `LIVEKIT_URL`, `LIVEKIT_API_KEY`, `LIVEKIT_API_SECRET` | The LiveKit server relays with calls enabled use |
| `BLOSSOM_S3_ENDPOINT`, `BLOSSOM_S3_REGION`, `BLOSSOM_S3_BUCKET`, `BLOSSOM_S3_ACCESS_KEY`, `BLOSSOM_S3_SECRET_KEY` | The bucket relays with Blossom enabled store media in |
| `STRIPE_SECRET_KEY` | The Stripe account tenants pay |

Every variable without a default is required, except `ADMIN_PUBKEYS` and the robot's profile.

## Deployment

Caravel ships as a Docker image built from the [`Dockerfile`](./Dockerfile) and published as `gitea.coracle.social/coracle/caravel` by [CI](./.gitea/workflows/ci.yml) on each push to `master`. One container runs the API on port `2892` and the dashboard on port `3000`. The dashboard's `VITE_` settings are filled in from `API_URL`, `RELAY_DOMAINS`, `PLATFORM_NAME` and `PLATFORM_LOGO` when the container starts.

Zooid runs on its own server behind [Caddy](https://caddyserver.com/), whose [on-demand TLS](https://caddyserver.com/docs/automatic-https#on-demand-tls) asks `GET /domains/check?domain=` before issuing each certificate, so only hosts some relay answers on get one:

```
{
  on_demand_tls {
    ask http://<caravel-host>:2892/domains/check
  }
}

:443 {
  tls {
    on_demand
  }
  reverse_proxy localhost:3334
}
```

Point a wildcard DNS record for each of the `RELAY_DOMAINS` at the zooid server. Tenants point a custom domain at their relay with a CNAME to its canonical host, which Caravel verifies in the background.

## Local development

You need [Rust](https://rustup.rs/), [Bun](https://bun.sh/), [just](https://github.com/casey/just), [onchange](https://www.npmjs.com/package/onchange) and [podman](https://podman.io/).

```sh
just pg
cp .env.template .env
cp frontend/.env.template frontend/.env
cd frontend && bun install && cd ..
just dev
```

`just pg` starts Postgres on `127.0.0.1:55433`. `just dev` runs the API on `http://127.0.0.1:2892` and the dashboard on `http://127.0.0.1:5173`, restarting the API when a crate changes.

`just check` formats, lints, builds and tests everything, as CI does. Tests create one database per test binary on the server `TEST_DATABASE_URL` names, `just pg`'s by default.
