---
name: backend
description: Architecture and conventions for the Caravel backend — an Axum + SQLite (sqlx) Rust service (edition 2024) with two long-running reactors. Covers the flat module map and where new code goes; the free-function query/command data layer (no repository objects) over a OnceLock global pool; the commit-then-publish activity-broadcast model the relay-sync and billing reactors hang off; auth that is structural (the AuthedPubkey NIP-98 extractor authenticates, but each handler must call require_admin/require_tenant itself — there is NO router-level authz, so a forgotten check fails OPEN); the web.rs response envelope; the env OnceLock singleton (every var required, panics at boot); the leaf integration wrappers (Stripe/NWC/Coinbase/Nostr/zooid) that billing.rs is the primary orchestrator for (though route handlers also call Stripe/Robot directly); and the clippy+build verification gate (prefer the backend-only just recipes over `just check`, which also compiles the frontend). Use this whenever working anywhere in backend/ — adding an endpoint, query, write, model, migration, config var, integration, or reactor — to follow house conventions and avoid fail-open auth, double-billing, and publish-before-commit traps.
---

# Caravel backend

This is the map of the Caravel backend: an Axum HTTP service plus two long-running reactors, persisting to SQLite via sqlx (the crate is edition 2024). It explains how the backend is organized and *why*, and points you at the modules for the *how* — reach for this for orientation and conventions, and reach for the modules it names for implementation detail. The deep, lookup-style material lives in `references/`.

One warning up front, because it is the single most dangerous wrong assumption you can carry into this codebase: **there is no router-level authorization.** Adding the `AuthedPubkey` extractor to a handler only proves *identity* — that the caller signed a valid NIP-98 event. If a handler then forgets to call a `require_*` helper, it is authenticated-but-open to *any* signed-in pubkey. Auth here fails **open**, not closed, so authorization is each handler's own explicit responsibility (`api.rs:13-15,101-137`).

Two more silent traps the body expands on, named here so you carry them in: `publish()` must happen **after** the transaction commits (never inside `with_tx`), or the reactors observe rows that might roll back; and double-billing is prevented by atomic guards rather than naive read-then-write — per-activity charges use a conditional `UPDATE ... WHERE billed_at IS NULL` claim (checking `rows_affected`) backed by a `UNIQUE` index on `invoice_item.activity_id`, while monthly renewals (whose items have `activity_id = NULL`) use a transaction-scoped read-then-write guard on the tenant's `renewed_at` marker re-read inside the same transaction.

## It's free functions and a global pool, not services/repositories

Internalize the actual shape before reaching for defaults, because the wrong default here (a `Service` or `Repository` holding a connection handle) is exactly what an agent reaches for:

- **There are no service/repository objects holding a pool or connection.** Data access is two modules of free functions — `query.rs` (all reads/SELECTs) and `command.rs` (all writes). Reads call `db::pool()` directly; writes either call `pool()` directly for single-statement updates or, for anything multi-step, run through the `db::with_tx` helper and operate on a `&mut Transaction` — instead of threading a handle through call sites (`query.rs:58-262`, `command.rs:14-705`, `db.rs:38-40,56-68`).
- **The pool and the activity broadcast channel are process-wide globals** in `OnceLock`s (`POOL`, `NOTIFY`), set once by `db::init()` at startup; reading before init or setting twice panics. This is deliberate — it is what lets `query`/`command` stay free functions instead of carrying a handle (`db.rs:15-54`).
- **The shared, application-scoped service container is `Api`** (it holds `billing`, `stripe`, `robot`). It is constructed once, wrapped in `Arc` in `Api::router()`, and installed as axum router state; handlers receive a cheaply-cloned reference as `State<Arc<Api>>` (the per-request cost is just a refcount bump, not a new instance). It is a thin authorization-and-orchestration surface, not a data handle (`api.rs:50-99`).
- **The crate is edition 2024**, which is required for the let-chains (`&&`-joined `let` patterns) in `infra.rs`. The `async |tx| { ... }` closures the `with_tx` callers use are *not* edition-gated — they were stabilized in Rust 1.85 and only need a recent toolchain, not edition 2024 specifically (`Cargo.toml:2-4`).

## The module map and where new code goes

The module map is flat under `backend/src` — one job per module: `api`, `billing`, `bitcoin`, `clock`, `command`, `db`, `env`, `infra`, `models`, `query`, `robot`, `routes`, `stripe`, `wallet`, `web`. `backend` is a dual library+binary crate, so this same set of modules is declared in two roots: `lib.rs` declares them as `pub mod` (the library root, the public/canonical declaration) and `main.rs` re-declares them as private `mod` for the binary entry point (`lib.rs:1-14`, `main.rs:1-14`).

The layering, so you know the call direction: a route handler performs authorization via `Api` helpers (`require_admin` / `require_admin_or_tenant` / `require_tenant`) when needed, then calls `query` (reads) / `command` (writes) / `billing` (orchestration), which call `db`. The integration leaves (`stripe`/`wallet`/`bitcoin`/`robot`) are composed in two places: `billing.rs` holds its own `stripe`/`wallet`/`robot` for the reconciliation loop, while `Api` holds `stripe` and `robot` that route handlers invoke *directly* (e.g. `create_tenant` calls `api.robot.fetch_nostr_name` and `api.stripe.create_customer`; `create_stripe_session` calls `api.stripe.create_portal_session`) — so the leaves are not composed exclusively by billing (`routes/tenants.rs:76-94,263-280`, `billing.rs:25-33`).

Where a *new* thing goes:

- **An endpoint** → a handler fn in the matching `routes/*.rs` **and** a `.route(...)` line in `Api::router()`. Both files are required; "I added a handler but the route 404s" is the number-one gotcha here because the two live in different files (`api.rs:66-99`).
- **A read** → a free async fn in `query.rs`.
- **A write** → a free fn in `command.rs` (a single-statement write runs directly on `pool()`; a multi-step write that must be atomic is composed inside `db::with_tx` and publishes its `Activity` after commit).
- **A model or field** → `models.rs`, plus a numbered migration under `migrations/` (pre-release the change is squashed into the current `0001_init.sql` rather than appended).
- **A config var** → `env.rs`.
- **A third-party call** → the matching leaf module (`stripe`/`wallet`/`bitcoin`/`robot`, or the zooid sync in `infra.rs`).

The full per-module responsibility table, the exact `main()` bootstrap order, and the `lib.rs`/tests note are in [references/module-map-and-layering.md](references/module-map-and-layering.md).

## Request lifecycle: authenticate structurally, authorize explicitly, return an envelope

**Authentication is structural, and there is no middleware.** Adding the `AuthedPubkey(auth)` param to a handler *is* the entire auth mechanism — it is a NIP-98 `FromRequestParts` extractor, and its mere presence makes the route require a signed-in caller. Omitting it makes the route public; the public routes (`GET /plans`, `GET /plans/:id`) simply omit it (`api.rs:206-223`, `routes/plans.rs:9-16`).

**Authorization is the handler's explicit job** via `Api` helpers: `require_admin` / `require_tenant` / `require_admin_or_tenant` (403 on failure) and `get_tenant_or_404` / `get_relay_or_404` (load-or-404). Restating the fail-open *why*: identity is not permission, and the router gates nothing, so a handler that authenticates but never authorizes is open to any signed-in pubkey (`api.rs:103-153`).

The handler shape is fixed: params ordered `State<Arc<Api>>` → `AuthedPubkey(auth)` → `Path`/`Query`/`Json`; the body returns `web::ApiResult`; wrap infra/db/external errors with `.map_err(internal)?`; let `require_*`/`get_*_or_404` propagate with a bare `?`; tail with `ok(..)`/`created(..)` (`routes/tenants.rs:61-71,121-141`).

One ordering rule with a security reason. For a path-by-id resource owned by a tenant (a relay, an invoice), **fetch first, then authorize** against the loaded resource's `tenant_pubkey` — you need the row to know whose it is, and this intentionally returns 403 (not 404) to a non-owner of an *existing* resource. For tenant routes keyed by the tenant's own pubkey, the `Path` *is* the `tenant_pubkey`, so authorize on it first (`routes/relays.rs:29-37`, `routes/invoices.rs:19-32`, `routes/tenants.rs:61-71`).

The response envelope: success goes through `web::ok`/`created`/`res`, returning `{ data, code: "ok" }`; errors go through typed builders returning `{ error, code }`. Note the keys differ — `data` on success, `error` on failure. `unauthorized`/`forbidden`/`not_found`/`internal` hardcode their code; `bad_request`/`unprocessable` take a caller-supplied kebab-case domain code. Translate sqlite `UNIQUE` violations to 422 via `map_unique_error` rather than letting them 500 (`web.rs:31-129`, `routes/relays.rs:309-316`).

Flag one deliberate weakness so nobody "fixes" it: the NIP-98 check here is a **session-style variant**. It verifies kind 27235, the signature, and that the last `u` tag equals `SERVER_URL`, but it does **not** bind HTTP method/URL/query, payload hash, timestamp freshness, or keep a replay cache — a valid header is effectively a ~10-minute bearer token. This is intentional (fewer signing prompts); do not add per-request binding (`api.rs:157-203`, `README.md:128-137`).

The exact decode steps, the envelope field shapes, and the full in-use domain-error-code list are in [references/request-lifecycle-and-web.md](references/request-lifecycle-and-web.md).

## The data layer: query/command split, transactions, and the activity log

Reads live in `query.rs` (mostly free async fns over `db::pool()`; `list_plans`/`get_plan` are synchronous). Writes live in `command.rs`: simple single-row writes are free async fns over `db::pool()`, but multi-step writes run inside `with_tx()` and delegate to private `_tx` helpers taking `&mut Transaction`. Tenant-scoped reads take a `tenant_pubkey` param and filter on it; some are suffixed `_for_tenant` (`list_relays_for_tenant`, `list_invoices_for_tenant`) but several are not (`list_open_invoices`, `list_unbilled_invoice_items`, `list_billable_activity`), so the suffix is not a reliable marker of tenant scoping (`query.rs:89-96,165-218`, `command.rs:14-87`).

`with_tx` is the only transaction primitive: it runs an async closure with a `&mut Transaction`, commits on `Ok`, and rolls back **only** via `Transaction`'s `Drop` on `Err` — there is no explicit rollback. The consequence to respect: a closure that swallows an error and returns `Ok` will commit a partial write. Multi-step atomic writes compose private `*_tx` helpers (each taking `&mut Transaction` as its first param) inside one `with_tx` closure (`db.rs:60-68`, `command.rs:466-704`).

The core idiom is the **activity log plus commit-then-publish**: a mutation records an `Activity` row inside the transaction, the `*_tx` helper *returns* that `Activity`, and the public command calls `db::publish(activity)` **after** `with_tx` commits — so reactors only ever observe durable rows. Publishing inside the transaction is the trap, because a subscriber could then act on a row that rolls back (`command.rs:179-182`, `db.rs:47-54`).

**Idempotency and double-billing are prevented by atomic guards, not naive read-then-write checks,** and the guard differs by path. Per-activity charges use a conditional claim: `mark_activity_billed_tx` updates `WHERE billed_at IS NULL` and returns a bool (`rows_affected() > 0`) you must honor, backstopped by `UNIQUE(invoice_item.activity_id)`. Other monotonic flips guard on null markers: `mark_invoice_paid_tx` only flips while `paid_at IS NULL AND voided_at IS NULL`. Renewals are the exception — their line items have `activity_id = NULL`, so neither the WHERE-guard nor the UNIQUE index protects them; their sole protection is a transaction-scoped read-then-write that re-reads `renewed_at` inside the same `with_tx` and only writes if the period hasn't been renewed (`command.rs:279-335,563-630`, `migrations/0001_init.sql:111-112`).

Billing-lifecycle entities model state as **nullable timestamp markers, not status enums**: an invoice is open while `paid_at` and `voided_at` are both null, a tenant is churned once `churned_at` is set, a bolt11 is settled once `settled_at` is set. Filter on the timestamps; these billing tables have no status column. Relay status is the one exception — a free-form `TEXT` column (active/inactive/delinquent) with no `CHECK` and no Rust enum, guarded only by the `RELAY_STATUS_*` consts and filtered/branched on throughout the relay code (`models.rs:4-6,54-60,120-142`, `migrations/0001_init.sql:32,48-60`).

**Read the time through `clock::now()`, never the wall clock.** Every timestamp the backend writes goes through it, and the e2e tests pin it. A direct `chrono::Utc::now()` escapes the pinned clock without any error (`clock.rs`).

Two more cross-cutting gotchas: boolean-ish columns are stored and typed as `i64` 0/1 (`policy_public_join`, the `*_enabled` flags, `synced`), not Rust `bool` — compare against 0/1; and plans are **not** a DB table (`list_plans`/`get_plan` are hardcoded, synchronous in-memory data), so adding a plan is a code edit, not a migration (`models.rs:86-94`, `query.rs:20-54`).

The per-table read helpers, the `Snapshot` enum, the strict-`<` historical lookups, the schema-squash migration rule, and the FK naming convention are in [references/data-layer-and-schema.md](references/data-layer-and-schema.md).

## Background reactors and the broadcast model

Two detached tokio tasks are launched from `main()` after `db::init()` and run for the life of the process alongside the axum server: `billing.start()` (time-driven) and `infra::start()` (event-driven) (`main.rs:56-67`).

**Billing is the hourly poller.** A tokio interval loop calls `reconcile_subscriptions()`, which sweeps all tenants and logs-and-continues per tenant so one failure never aborts the sweep. The same `reconcile_subscription(tenant, attempt_payment)` is shared by the worker (`true`) and the synchronous reconcile route (`false`) — parameterize shared reconciliation rather than duplicating it (`billing.rs:46-130`).

**Infra is the broadcast reactor, and the model for any new background reaction.** It calls `db::subscribe()` to the activity channel, runs a reconcile sweep on *startup* to recover work missed while the process was down, then loops on `recv()`. It must handle `RecvError::Lagged` by running a full reconcile sweep over the DB "pending" query — the channel is best-effort with capacity 64, so you cannot assume you saw every message; `Closed` ends the worker (`infra.rs:21-44`).

The two non-negotiable reactor rules, with their *why*:

- **The top-level reactor driver loops never crash the process on a failure.** The billing poll loop, the per-tenant sweep, and the infra `recv` loop each wrap their unit of work in a `tracing::error!`-logged guard with structured fields and continue (`sync_relay` is even infallible by design). Note this catch-and-continue is only at the driver level: inner batch loops (e.g. the per-activity loop, `reconcile_renewal`, `reconcile_relay_state`) propagate a per-item error via `?` and abandon the rest of the current batch — that error still bubbles up to the nearest wrapped driver, so the process stays alive, but the failing item aborts its enclosing batch rather than being skipped (`billing.rs:52-54,66-74,104-110`, `infra.rs:28-44,83-89`).
- **Correctness comes from the DB reconcile sweep, never from one-message-per-event.** `db::publish` silently drops when there are no subscribers, and the bounded channel drops on lag — the broadcast is a hint, the DB "pending" query is the source of truth (`db.rs:50-54`, `infra.rs:35-41`).

New background reactions should hook the publish/subscribe activity stream rather than adding a new poller; tuning knobs (poll interval, grace/DM windows, retry base/max/attempts) live as module-level consts at the top of the worker file (`db.rs:43-54`, `billing.rs:15-23`, `infra.rs:15-17`).

The relay-sync retry/backoff mechanics, the self-feeding `fail_relay_sync` loop, the POST-vs-PATCH `is_new` logic, the secret-never-stored detail, and the full billing dunning cascade are in [references/reactors-and-relay-sync.md](references/reactors-and-relay-sync.md).

## External integrations: Nostr, Stripe, Lightning/NWC, Coinbase, zooid

Every integration is a **leaf I/O wrapper** that speaks only to the third party, returns `anyhow::Result`, and knows nothing about the DB, routes, or domain (`stripe.rs`'s only `crate` import is `env`, for the API key). `stripe.rs` parses Stripe's JSON internally via a private `send_json -> Result<serde_json::Value>` helper, but its public methods hand back small typed results (e.g. `Result<String>`, `Result<Option<String>>`), not raw `serde_json::Value`. New external calls go in the matching leaf module (`stripe`/`wallet`/`bitcoin`/`robot`, or the zooid sync in `infra.rs`), never inline in a route or mixed with DB logic (`stripe.rs:1-5`, `wallet.rs:7-13`).

`billing.rs` is the **primary orchestrator** that composes integrations against the DB — notably the payment cascade (NWC auto-pay → out-of-band lightning check → Stripe card on file → manual DM), where a failing NWC or Stripe attempt records its error on the tenant but never aborts the cascade and the first success returns early. It is *not* the only place integrations are used: route handlers also call Stripe and the robot directly (e.g. `create_tenant` calls `api.robot.fetch_nostr_name` and `api.stripe.create_customer`; `create_stripe_session` calls `api.stripe.create_portal_session`). And a handler may invoke more than one billing method — `reconcile_tenant` calls both `sync_stripe_customer` and `reconcile_subscription`, `reconcile_invoice` calls both `ensure_bolt11_for_invoice` and `attempt_payment` — and those public billing methods are themselves orchestrators that fan out internally (`billing.rs:29-33,326-377`, `routes/tenants.rs:84-94,182-190`).

Sensitive at-rest values (a tenant's `nwc_url`) are NIP-44 **self-encrypted** with the robot's own keypair via `env.encrypt`/`decrypt` — at-rest confidentiality for the service, *not* a DM to the tenant — encrypted at the write boundary (the route) and decrypted only at point of use (billing). Outbound zooid calls are NIP-98 signed via `env.make_auth` (`env.rs:86-107`, `routes/tenants.rs:130-137`).

Two design intentions not to "fix": an off-session Stripe `PaymentIntent` is treated as **failed unless `status == "succeeded"`**, so the cascade falls through via two distinct paths — an off-session 3DS/authentication demand returns an HTTP 402 error caught earlier by `error_for_status`, while a 2xx response whose status is merely not `"succeeded"` is caught by the explicit status check (do not assume 3DS "comes back 2xx" — for off-session confirmed intents Stripe surfaces it as an HTTP error); and the zooid relay secret is generated fresh and sent **only on first sync** (`is_new`), so Caravel never stores relay secrets, which is why a re-sync must `PATCH`, not `POST` (`stripe.rs:104-106,135-143,194-227`, `infra.rs:168-243`).

The Stripe idempotency-key HMAC scheme, the currency-minor exponent table, Robot's publish-on-construct side effect, the relay-list cache TTL, and the per-integration error-string conventions are in [references/integrations.md](references/integrations.md).

## Config: the env singleton

All config is one process-wide `Env` struct in a `static OnceLock`, loaded once by `env::init()` in `main()` immediately after `dotenv`, *before* `db::init()` — the env → db → services order is load-bearing. Read config **only** through `crate::env::get()` (which returns `&'static Env`); never read `std::env::var` outside `env.rs` (`env.rs:8-20`, `main.rs:28-37`).

**Every variable is required** — there are no optional vars and no graceful degradation. `require_str`/`require_u16`/`require_csv` panic at boot on a missing, blank, or invalid value (and an invalid `ROBOT_SECRET` panics too). Adding an integration var without setting it crashes the process on boot rather than degrading (`env.rs:110-140`).

Adding a config var is **four coordinated edits**: a field on `Env`, a load line in `Env::load` with the right `require_*` helper, README docs, and `.env.template`. Do crypto/auth through `Env` methods (`encrypt`/`decrypt`, `make_auth`), not by reaching for the keys ad hoc (`env.rs:22-84`).

Two traps. NIP-98 host-affinity means `SERVER_URL` must *exactly* equal the client's `u` tag or every authenticated request 401s. And the README uses stale var names that don't exist in `env.rs`: its local-dev table lists `ADMINS` (real name `SERVER_ADMIN_PUBKEYS`) and `ZOOID_API_SECRET` (the backend has no such var — it consumes `ZOOID_API_URL` and signs zooid requests with `ROBOT_SECRET`), while the production `docker run` example sets `PLATFORM_NAME` (a frontend VITE var, not a backend `Env` field) — trust `env.rs` and `.env.template`, not the README (`api.rs:158-202`, `README.md:19,101-102` vs `env.rs:60,76`).

The full variable surface, the `DATABASE_URL`/`CARGO_MANIFEST_DIR` rewrite, and the CORS silent-drop are in [references/config-and-env.md](references/config-and-env.md).

## Building and verifying a change

The `justfile` is the canonical task runner; backend recipes `cd` into `backend/` and run one cargo command. The minimal diff-safe gate for a backend edit is `just build-backend` (`cargo build`) plus `just lint-backend` (`cargo clippy --all-targets -- -D warnings`, where every warning is a hard error), plus `just test-backend` if the touched area has tests (`justfile:17-30`). Fix warnings in code you touch rather than churning unrelated code to silence them.

**For a backend-only change prefer the backend-scoped recipes (`just fmt-backend lint-backend build-backend test-backend`) over a full `just check`.** `just check` also runs against the frontend — `build` is `build-backend build-frontend` — so it compiles the frontend even for a backend-only edit (`justfile:39,43`).

CI (`.gitea/workflows/ci.yml`) runs `cargo fmt --check`, the same clippy command as `just lint-backend`, and `cargo test --locked` on every pull request and push to `master`. It pins Rust 1.94, the toolchain the Dockerfile builds production with. Your local toolchain may be newer, so code that uses a newer API compiles locally and fails in CI. It also builds the frontend, and a separate job publishes the Docker image only for a push to `master` after the backend and frontend checks pass.

The backend's tests are end-to-end billing scenarios in `backend/tests/billing.rs`. Each one replays a tenant across several billing periods against a real, migrated SQLite database, with the clock pinned. The only other tests are unit tests for `SERVER_ALLOW_ORIGINS` matching in `env.rs`, and there are no route-level tests.

The scenarios call `command` and `billing` directly because `POST /tenants`, which every scenario would start with, calls Stripe. Route-level behavior (auth, validation, the response envelope) belongs in tests under `api::tests`, which `just test-backend-api` runs. Those tests drive the `Router` through tower's `oneshot`, using the `tower`/`util` dev-dependency.

Scenarios use the harness in `backend/tests/common/mod.rs`, and follow three rules:

- **Wrap each test in `run(...)`, not `#[tokio::test]`.** The database pool is a process-wide global that sqlx binds to the runtime that opened it, so every test shares one runtime. Tests run one at a time, because the pinned clock is global too.
- **Pay the invoices.** An open invoice churns its tenant seven days later, which pauses their relays and voids their credits. Use `reconcile_and_pay`, and plain `reconcile` only in scenarios about not paying.
- **Write expected amounts out with their derivation** rather than recomputing them through the proration code, so that a change to that code fails a test.

## House style (brief)

- **Comments are minimal**, one line where possible; a doc comment states a function's *purpose*, not its implementation. There is one canonical place for any fact — model/field semantics in `models.rs` doc comments only, DB index rationale in migration SQL comments — so don't duplicate across layers (root `AGENTS.md:3-12`).
- **Naming.** FK columns are `{model}_{pk}` (`relay.tenant_pubkey`, and in `models.rs` `invoice_item.activity_id`, `bolt11.invoice_id`); a tenant's pubkey is `tenant_pubkey` except in already-tenant-scoped contexts like `tenant.pubkey` or `get_tenant(pubkey)` (root `AGENTS.md:16,18`). Separately, some tenant-scoped query/command fns are suffixed `_for_tenant` (a codebase convention in `query.rs`/`command.rs`, not stated in `AGENTS.md`; see the data-layer reference for why it is not a reliable marker).
- **Rust idioms.** Prefer `&str` over `&String` params; avoid passing `&mut` into functions — return results and let the caller manage mutability; resist over-DRY — extract only for a distinct concern, 3+ repetitions, or genuine clarity (the inline zooid body and the longhand `update_relay` merge are deliberate) (root `AGENTS.md:30,32,34`).
- **Markdown.** Do not hard-wrap at a fixed column — write one logical line per paragraph (root `AGENTS.md:24-26`).
