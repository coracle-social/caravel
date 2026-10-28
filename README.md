# Caravel

A multi-tenant platform for hosting Nostr community relays. Tenants create relays, choose their plans and settings, and pay for them, and the platform bills them monthly. One process serves every relay on its own hosts, with its rooms, media, push notifications and calls, and sells memberships of relays that charge for them, paying their tenants through Stripe Connect.

## Layout

The backend is a Rust workspace whose crates are layered, each depending only on those listed before it. The vocabulary is in [DOMAIN.md](./DOMAIN.md).

- `crates/util` — the foundations: the errors, configuration, clock, change bus, serde helpers, hex and hashing, the Postgres pool, migrations and transactions, and clients for Stripe, Nostr relays and keys, Nostr Wallet Connect, the robot's Nostr identity, DNS, media storage, LiveKit and outbound HTTP
- `crates/domain/relays` — the relay itself: NIP-11 profiles, access, NIP-86 management, rooms, media, events, calls, transfer, notifications and the policy rules
- `crates/domain/billing` — tenants and relays as billable entities: plans, billing periods, invoices, payouts, usage, and the memberships relays sell
- `crates/domain/operations` — what operators need to run the platform: the moderation feed and web sessions
- `crates/core` — the adapter between the services and the domain: the `Core` handle and the subsystems (hosting, management, ingest, memberships, billing and the rest) that run the domain's rules against it
- `crates/services/relay` — the virtual relays: websockets, NIP-11 and NIP-86, Blossom, push, LiveKit tokens and webhooks, and resolving hosts to relays
- `crates/services/api` — the JSON API, documented in [crates/services/api/README.md](./crates/services/api/README.md)
- `crates/services/worker` — when the background jobs run, which live in the core
- `crates/app` — the `caravel` binary, which reads the environment, serves each request by its host and runs the services

`web/` builds three SolidJS apps, which the binary embeds: the tenant dashboard, the admin dashboard, and the member portal each relay serves on its own host.

## Configuration

The binary reads these from the environment, or from a `.env` file in its working directory. [`.env.template`](./.env.template) has local defaults.

| Variable | Description |
| --- | --- |
| `PORT` | The port everything is served on, `2892` by default |
| `API_URL` | The API's public base url, which the NIP-98 signature that starts a session is checked against. On its own host, the API is served at the root, as Flotilla needs; sharing `PLATFORM_URL`'s host, it needs a path such as `/api` |
| `PLATFORM_URL` | The dashboards' public url. Without it, no dashboard is served |
| `PLATFORM_NAME`, `PLATFORM_LOGO` | The name and logo the web apps show, `Caravel` and `/caravel.png` by default |
| `ADMIN_PUBKEYS` | Comma-separated hex pubkeys of the platform's admins |
| `DATABASE_URL` | A Postgres url |
| `ROBOT_SECRET` | The robot's hex Nostr secret key. It signs the robot's DMs and encrypts stored secrets, relays' keys among them |
| `ROBOT_NAME`, `ROBOT_DESCRIPTION`, `ROBOT_PICTURE` | The robot's profile, all optional |
| `ROBOT_WALLET` | The platform's NWC url, which issues the Lightning invoices tenants pay |
| `ROBOT_OUTBOX_RELAYS` | Relays the robot publishes its profile and relay lists to |
| `ROBOT_INDEXER_RELAYS` | Relays the robot looks up recipients' relay lists and profiles on, and publishes its own to |
| `ROBOT_MESSAGING_RELAYS` | Relays the robot receives DMs on |
| `RELAY_DOMAINS` | Comma-separated domains relay subdomains live under |
| `LIVEKIT_URL`, `LIVEKIT_API_KEY`, `LIVEKIT_API_SECRET` | The LiveKit server relays' calls run on. Without them no relay has calls |
| `MEDIA_DIR` | The directory relays' media is kept in when no bucket is set, `data/media` by default |
| `MEDIA_S3_BUCKET`, `MEDIA_S3_REGION`, `MEDIA_S3_ACCESS_KEY`, `MEDIA_S3_SECRET_KEY`, `MEDIA_S3_ENDPOINT` | The S3 bucket relays' media is kept in instead; the endpoint is needed only for a service other than AWS |
| `STRIPE_SECRET_KEY` | The Stripe account tenants pay, and through whose Connect platform members pay tenants |
| `MEMBER_FEE_PERCENT` | The platform's cut of each membership payment, `10` by default, with at most two decimal places |
| `FLOTILLA_URL` | The Flotilla instance invites are accepted in, `https://app.flotilla.social` by default |
| `FORWARDED_HEADER` | The header a trusted proxy reports a client's address in, such as `X-Forwarded-For` |
| `CONNECTIONS_PER_MINUTE` | New relay connections allowed per minute from one address, `0` (unlimited) by default |

Every variable without a default is required, except `ADMIN_PUBKEYS`, the robot's profile, `FORWARDED_HEADER`, `PLATFORM_URL`, LiveKit's and the bucket's.

## Deployment

Caravel ships as a Docker image built from the [`Dockerfile`](./Dockerfile) and published as `gitea.coracle.social/coracle/caravel` by [CI](./.gitea/workflows/ci.yml) on each push to `master`. The image is one binary, with the web apps built in, serving everything on port `2892`: each request is answered by its host, so the platform host gets the dashboards, the API host the API, and a relay's hosts that relay and its member portal. Mount a volume at `/app/data` for media kept on disk.

Every host reaches Caravel through [Caddy](https://caddyserver.com/). The platform and API hosts get ordinary certificates; relay hosts use [on-demand TLS](https://caddyserver.com/docs/automatic-https#on-demand-tls), which asks `GET /domains/check?domain=` before issuing each certificate, so only hosts some relay answers on get one:

```
{
  on_demand_tls {
    ask http://<caravel-host>:2892/domains/check
  }
}

<platform-host>, <api-host> {
  reverse_proxy <caravel-host>:2892
}

:443 {
  tls {
    on_demand
  }
  reverse_proxy <caravel-host>:2892
}
```

Point a wildcard DNS record for each of the `RELAY_DOMAINS` at the Caddy server. LiveKit's webhook is `https://<any relay host>/.well-known/nip29/livekit/webhook`, which serves every relay's calls. Tenants point a custom domain at their relay with a CNAME to its canonical host, which Caravel verifies in the background.

## Local development

You need [Rust](https://rustup.rs/), [Node.js](https://nodejs.org/) 22.13 or later with [pnpm](https://pnpm.io/), [just](https://github.com/casey/just), [onchange](https://www.npmjs.com/package/onchange) and [podman](https://podman.io/).

```sh
just pg
cp .env.template .env
cp web/.env.template web/.env
cd web && pnpm install && cd ..
just dev
```

`just pg` starts Postgres on `127.0.0.1:55433`, and `just reset-db` recreates its database, which a change to the schema needs until the first release. `just dev` runs the API on `http://127.0.0.1:2892` and the web apps on `http://127.0.0.1:5173`, restarting the API when a crate changes. There, `/admin` is the admin dashboard and `/portal?relay=<id>` a relay's member portal. To serve the built apps from the binary instead, run `just build-web` and set `PLATFORM_URL`.

`just check` formats, lints, builds and tests everything, as CI does. Tests create one database per test binary on the server `TEST_DATABASE_URL` names, `just pg`'s by default.
