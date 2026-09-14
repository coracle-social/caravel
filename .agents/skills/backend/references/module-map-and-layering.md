# Module map and layering (deep detail)

This is the lookup-depth companion to the SKILL.md "module map" section: the full per-module responsibility map, the exact `main()` bootstrap and spawn order, the layering direction with concrete traces, and the test-target note. It exists so the SKILL.md "where things live" section can stay prose-only.

## The flat module map

Everything lives flat under `backend/src`, one job per module. `backend` is a dual library+binary crate, so this same set of modules is declared in two roots: `lib.rs` declares them as `pub mod` (the library root, the public/canonical declaration) and `main.rs` re-declares them as private `mod` for the binary entry point:

- **`api`** — the router, the `Api` service container, the authorization helpers (`require_*`, `get_*_or_404`, `is_admin`), and the `AuthedPubkey` NIP-98 extractor.
- **`billing`** — the orchestrator: it composes the integration leaves against the DB to reconcile activity into invoice items, renew subscriptions, and collect payment.
- **`bitcoin`** — the Coinbase fiat↔msats conversion leaf.
- **`clock`** — the source of the current time for every timestamp the backend writes; tests pin it.
- **`command`** — all writes, as free async fns: single-statement writes run directly on `db::pool()`, while multi-step writes run inside `db::with_tx`.
- **`db`** — the global `SqlitePool`, the activity broadcast channel, and `with_tx`.
- **`env`** — the config singleton.
- **`infra`** — the relay-sync reactor plus the zooid client.
- **`models`** — the domain and sqlite-row structs plus the relay status string constants.
- **`query`** — all reads, plus the hardcoded plans.
- **`robot`** — the service's Nostr identity and DM sender.
- **`routes/*`** — the HTTP handlers, grouped by resource (`identity`, `plans`, `tenants`, `relays`, `invoices`).
- **`stripe`** — the Stripe HTTP leaf.
- **`wallet`** — the NWC (Nostr Wallet Connect) leaf.
- **`web`** — the response envelope and its success/error builders.

Source: `lib.rs:1-14`, `main.rs:1-14`, and each module head.

## The `main()` bootstrap order

The order is strict, and each service is built from the ones before it:

1. `dotenvy::dotenv().ok()`
2. tracing setup
3. `env::init()` — loads and validates all config, panicking on any missing var
4. `db::init().await` — normalizes the sqlite URL, opens the pool, sets WAL, runs migrations, and installs the broadcast channel
5. `Robot::new()`, then `robot.publish_identity().await` to announce the robot's Nostr identity (see the integrations reference)
6. `Stripe::new()`
7. `Billing::new(robot.clone())`
8. `Api::new(billing, stripe, robot)`

Then it builds the router (`api.router().layer(cors)`), spawns `infra::start()` and `billing.start()` as detached tokio tasks, and only then binds `127.0.0.1:{SERVER_PORT}` and calls `axum::serve`. The HTTP server and the two workers run concurrently for the life of the process.

Source: `main.rs:27-67`.

## The layering direction

The call direction is: route handler → (when needed) `Api` authorization helpers (`require_admin` / `require_admin_or_tenant` / `require_tenant`) → `query` (reads) / `command` (writes) / `billing` (orchestration) → `db`. There is no single strict linear layer: a handler may skip the authz helpers entirely, and it may call integration leaves directly rather than going through `billing`. The integration leaves (`stripe`/`wallet`/`bitcoin`/`robot`) are composed in **two** places — `billing.rs` holds its own `stripe`/`wallet`/`robot` for the reconciliation loop, while `Api` holds `stripe` and `robot` that route handlers invoke directly — so the leaves are *not* composed exclusively by `billing.rs`.

Two concrete traces:

- **`create_tenant`** calls no `require_*` helper (only the `AuthedPubkey` extractor); it calls `query::get_tenant`, then `api.robot.fetch_nostr_name` and `api.stripe.create_customer` directly (two integration leaves, not via `billing`), then `command::create_tenant` (`routes/tenants.rs:76-109`).
- **`reconcile_invoice`** calls `query::get_invoice`, then `billing.ensure_bolt11_for_invoice` and `billing.attempt_payment` — here the handler delegates the multi-integration orchestration to `billing` rather than calling the leaves itself (`routes/invoices.rs:35-60`).

## Router assembly

`Api::router()` is the single place every route string is wired to a handler fn imported from `routes/*`, after which `Arc<Api>` is attached via `.with_state`. An endpoint therefore needs *both* a handler fn (in the matching `routes/*.rs`) and a `.route(...)` line (in `api.rs`) — two different files, both required (`api.rs:66-99`).

## The test-target note

`lib.rs` re-exports every module `pub` for the integration tests under `backend/tests`, its only consumer. Because `main.rs` declares its own private copy of each module, a `pub` fn that only tests call is unused in the binary build and trips `dead_code`; `clock::pin` and `clock::release` carry `#[allow(dead_code)]` for this (`lib.rs`, `clock.rs`).

## Sources

- flat module list — `backend/src/lib.rs:1-14`, `backend/src/main.rs:1-14`, and each module head
- `main()` bootstrap and spawn order — `backend/src/main.rs:27-67`
- layering traces — `backend/src/routes/tenants.rs:76-109`, `backend/src/routes/invoices.rs:35-60`
- router assembly — `backend/src/api.rs:66-99`
- test-target note — `backend/src/lib.rs`, `backend/src/clock.rs`, `backend/tests/common/mod.rs`
