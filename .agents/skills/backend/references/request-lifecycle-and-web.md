# Request lifecycle and the web envelope (deep detail)

This is the lookup-depth companion to the SKILL.md "request lifecycle" section: the exact NIP-98 decode, the success/error envelope field shapes, every builder's fixed-vs-supplied code, the full in-use domain-error-code list, and the handful of authorization quirks. All of it lives in `api.rs`, `web.rs`, and the `routes/*` handlers.

## The NIP-98 decode, step by step

`decode_nip98_pubkey` does the following, and every failure collapses to a single 401 `unauthorized` via `extract_auth_pubkey`'s `.map_err(unauthorized)`:

1. require an `Authorization: Nostr <base64>` header
2. base64-decode it to a JSON Nostr event
3. assert `event.kind == HttpAuth` (kind 27235)
4. call `event.verify()` (the signature/id check)
5. take the **last** `u` tag (`.last()`, not `.first()`)
6. assert it equals `env::get().server_url`
7. return `event.pubkey.to_hex()`

Because all of these collapse to one 401, you cannot distinguish a missing header from a bad signature from a host mismatch at the response level. Source: `api.rs:163-203`.

## The deliberate non-strictness

The check binds signer identity and host affinity only. It does **not** verify the HTTP method, the exact request URL/path/query, a payload hash, timestamp freshness, or maintain any replay cache. Per the README rationale, the frontend signs one kind-27235 event with `u = VITE_API_URL` and caches the header ~10 minutes; the tradeoff is a reusable ~10-minute bearer header (fewer wallet-signing prompts, no cookie sessions) at the cost of weaker request-intent binding than strict NIP-98. Do not "fix" it to per-request binding — it is a design choice. Source: `api.rs:167-203`, `README.md:128-137`.

## The envelope structs

- **Success:** `DataResponse { data: T, code: "ok" }`, serialized as `{ "data": ..., "code": "ok" }`. The field is `data`, and `code` is a `&'static str`.
- **Error:** `ErrorResponse { error: String, code: String }`, serialized as `{ "error": ..., "code": ... }`. The field is `error`, and `code` is an owned `String`.

Note the top-level keys differ — `data` on success, `error` on failure — so a client must branch on success-vs-error rather than reading one fixed key. The only HTTP statuses the success builders emit are 200 (`ok`) and 201 (`created`); there is no 204/no-content builder in this file. Source: `web.rs:33-43`.

Success builders return `ApiResult` (= `Result<Response, ApiError>`) already wrapped in `Ok`, so they sit at the tail of a handler with no `Ok(..)`: `res(status, data)`, `ok(data)` (= `res(OK, ..)`), `created(data)` (= `res(CREATED, ..)`). `ApiError` is a boxed `Response` with the status baked in at construction — it carries no separate status field. Source: `web.rs:17-57`.

## Error builders and their codes

The named error builders fix both status and code: `unauthorized` → 401/`unauthorized`, `forbidden` → 403/`forbidden`, `not_found` → 404/`not-found`, `internal` → 500/`internal`. The two domain builders take a **caller-supplied** kebab-case code: `bad_request(code, msg)` → 400, `unprocessable(code, msg)` → 422. Passing the wrong status builder silently emits the wrong HTTP status with your intended code. Source: `web.rs:61-103`.

`map_unique_error` downcasts `sqlx::Error::Database` and matches the raw message *substring*: contains `pubkey` → `pubkey-exists`, contains `subdomain` → `subdomain-exists`, else `None`. Because it matches on the message text rather than the constraint name, a column rename can silently regress the 422 to a 500. `map_relay_write_error` is a **private** helper inside `routes/relays.rs` (not exported from `web.rs`) that wraps `map_unique_error`: a `subdomain-exists` hit becomes a 422, anything else a 500. Source: `web.rs:115-129`, `routes/relays.rs:309-316`.

## The full in-use domain-error-code list

Beyond the framework codes (`ok`/`unauthorized`/`forbidden`/`not-found`/`internal`), the domain codes actually surfaced to clients are: `subdomain-exists`, `invalid-subdomain`, `invalid-plan`, `premium-feature`, `member-limit-exceeded` (all 422), and `relay-is-active` / `relay-is-inactive` / `relay-is-delinquent` (all 400, via `bad_request`). `pubkey-exists` is *defined* in `map_unique_error` alongside `subdomain-exists` but is **not** surfaced as an error: its only call site (`routes/tenants.rs:105`) intercepts the unique-constraint violation and returns 200 OK with the existing tenant (idempotent re-fetch). Source: status helpers `bad_request` (400) / `unprocessable` (422) at `web.rs:89-95`, code strings at `web.rs:122-127` and `routes/relays.rs:204,225,230,249,253,283,287,293,311-312`, `pubkey-exists` interception at `routes/tenants.rs:103-110`.

## Load-vs-authorize ordering

For a path-by-id resource owned by a tenant, fetch the resource **first** (via `get_*_or_404`), then authorize against its `tenant_pubkey` — you need the loaded row to know whose it is. This also intentionally leaks existence: a non-owner of an *existing* relay/invoice gets a 403, and a 404 only for a truly missing id. For tenant routes keyed by the tenant's own pubkey, the `Path` *is* the `tenant_pubkey`, so authorize on it first, then fetch. Don't reorder these. Source: `routes/relays.rs:29-37`, `routes/invoices.rs:19-32`, `routes/tenants.rs:61-71`.

## Authorization quirks

- **`create_tenant`** authorizes nobody beyond authentication: it uses the `AuthedPubkey` *as* the tenant identity, so a caller can only ever create or return their own tenant. It is idempotent and even swallows a `pubkey-exists` race by re-reading; a missing row after that race is a 500 with "tenant row missing after unique-constraint race". Source: `routes/tenants.rs:76-114`.
- **`GET /tenants/:pubkey/stripe/session`** is the only same-tenant-*only* route (it uses `require_tenant`, not `require_admin_or_tenant`). Source: `routes/tenants.rs:263-269`.
- **`get_plan`** is synchronous: `query::get_plan` returns a `Result` (no `.await`, no `Option`), so a missing plan is mapped to `not_found` via an `Err`, not a `None`. Don't pattern-match plans for a `None` case. Source: `routes/plans.rs:13-15`.
- **`update_relay`** enforces `member-limit-exceeded` (422) only when the plan actually changes and the new plan has a `members` limit; it fetches live member counts from zooid, which returns empty for unsynced relays, so an unsynced relay appears to have 0 members for the limit check. Source: `routes/relays.rs:190-207,263-269`.

## Sources

- NIP-98 decode + non-strictness — `backend/src/api.rs:163-203`, `backend/README.md:128-137`
- envelope structs + success builders — `backend/src/web.rs:17-57`
- error builders + `map_unique_error` — `backend/src/web.rs:61-129`, `backend/src/routes/relays.rs:309-316`
- domain-error-code list — `backend/src/web.rs:89-95,122-127`, `backend/src/routes/relays.rs:204,224-296`, `backend/src/routes/tenants.rs:103-110`
- load-vs-authorize ordering — `backend/src/routes/relays.rs:29-37`, `backend/src/routes/invoices.rs:19-32`, `backend/src/routes/tenants.rs:61-71`
- authorization quirks — `backend/src/routes/tenants.rs:76-114,263-269`, `backend/src/routes/plans.rs:13-15`, `backend/src/routes/relays.rs:190-207,263-269`
