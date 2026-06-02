# Data and state lifecycle (deep detail)

This is the lookup-depth companion to the SKILL.md "data layer" and "global state" sections. It documents the exact mechanics of the API client, NIP-98 auth, the circular-import seam, the session/provisioning lifecycle, and the optimistic-mutation flow. All of it lives in `frontend/src/lib/api.ts`, `state.ts`, and `useRelayToggles.ts`.

## The API envelope and errors

`callApi<TRequest, TResponse>(method, path, body?)` is the single fetch wrapper (`api.ts`). It calls `makeAuth()`, builds the URL with `new URL(path, API_URL)`, sets `Content-Type: application/json` and an `Authorization` header *only when auth is present*, and JSON-stringifies the body — an `undefined` body sends no body at all.

Responses are unwrapped from an `ApiOk<T> = { data: T; code: string }` envelope: `callApi` returns `payload.data`, not the raw payload. A `204` returns `undefined` cast to `TResponse`.

On a non-2xx response it throws `new ApiError(message, response.status)`. `ApiError extends Error` and carries a numeric `status`. The message comes from the JSON body's `error` field when present, otherwise `Request failed (<status>)`.

`listPlans()` deliberately bypasses `callApi`: it does its own raw `fetch` with no auth and unwraps `{ data: Plan[] }` directly. It is the only wrapper that bypasses `callApi`, but not the only public/unauthed endpoint — `/plans` and `/plans/:id` are both public (their backend handlers omit the `AuthedPubkey` extractor); `getPlan` reaches its public route through `callApi`, which omits the `Authorization` header when logged out.

## NIP-98 auth and the 10-minute cache

`makeAuth()` builds a session-style NIP-98 header. It reads the active account via the injected getter (see the DI seam below), and if none is logged in it returns `undefined` — so `callApi` silently sends an *unauthenticated* request rather than failing fast, and authed endpoints then 401 from the backend.

When an account is present it signs a **kind-27235** event with empty content and a single `["u", API_URL]` tag, base64-encodes the JSON, and returns `Nostr <base64>`. The result is cached per-pubkey for **10 minutes** (`expiresAt = now + 10 * 60 * 1000`). Switching accounts invalidates the cache via the pubkey check. Within the cache window the event's `created_at` is fixed at sign time, so freshness relies on the backend's own NIP-98 window.

## The api.ts ↔ state.ts circular-import seam

`api.ts` needs the active account to build auth, but `state.ts` (which owns the `account` signal) imports from `api.ts` — a cycle. It is broken by dependency injection: `api.ts` holds a private module-level `getAccount` and exposes `registerAccountGetter(fn)`; `state.ts` calls `registerAccountGetter(account)` to inject the signal.

Side-effecting initialization in `state.ts` (localStorage hydration + the `accountManager.active$` subscription) is wrapped in `queueMicrotask(...)` to defer it past module evaluation and avoid temporal-dead-zone errors from the cycle. When adding new cross-module state that both files touch, follow the same pattern: a `register*Getter` injection plus deferred init.

## Session and lazy tenant provisioning

The tenant row is created lazily, which drives the whole session ordering.

`ensureSessionTenant()` is idempotent lazy provisioning: it reads the active pubkey, `await`s `createTenant()` (`POST /tenants`), then — only if the active pubkey is still the same — sets `billingPubkey` to unlock billing reads. The in-flight promise is shared per-pubkey via a module-level `tenantEnsure` object, so the login flow and the activation subscriber don't double-provision; `createTenant` is itself idempotent.

The `accountManager.active$` subscriber runs on **every account switch** and performs this ordering: `setAccount(account)` → persist accounts + active id to `localStorage` → `refetchIdentity()` → `setBillingPubkey(undefined)` to **lock** billing reads → `void ensureSessionTenant()` to re-unlock once provisioned. Locking first is what stops billing reads from firing against a not-yet-provisioned tenant during signup.

Login's `completeLogin` (`views/Login.tsx`) `await`s `ensureSessionTenant()` and rolls back via `accountManager.removeAccount(...)` if it throws — so a failed provision aborts the login.

This is why the billing resources (`billingTenant`, `billingInvoices`, `billingRelays`, `billingDraftInvoice`) are gated on a `billingPubkey` signal, not on `account()` directly. The gate matters because `getTenant` and `getDraftInvoice` (which use `get_tenant_or_404`) 404 before the row exists; `listTenantInvoices` and `listTenantRelays` instead return empty results, so the gate primarily protects the tenant/draft reads.

`refetchBilling()` refetches all four via `Promise.allSettled`, and on any rejected result it `console.error`s and toasts `"Failed to refresh billing data"`. It is the pure billing refresh that callers invoke after select billing-affecting flows (payment dialog/setup close, paid relay create, `autopayBilling`'s `finally` block) — it is **not** called after every billing-affecting mutation: relay deactivate/reactivate, plan toggles, and plan changes (including downgrade-to-free) in `useRelayToggles` only run the per-relay `refetch()`, and creating a free relay in `Home.tsx` skips it.

## The optimistic mutation flow

The canonical mutation shape (`useRelayToggles.ts`):

1. `mutate(next)` — optimistically push the new value into the resource.
2. `await updateRelayById(...)` — perform the API write.
3. `await refetch()` — reconcile against the server's truth.
4. On error: `mutate(previous)` to roll back, and `setToastMessage(e instanceof Error ? e.message : <fallback>)`.

`setToastMessage` defaults the variant to `"error"`; pass `"success"` for confirmations and `setToastMessage("")` to clear. Rethrow only when a caller needs to react (e.g. the plan-upgrade flow continues into `resolvePostPaidFlow()` for paid upgrades).

## Sources

- API client, envelope, `ApiError`, `listPlans` bypass — `frontend/src/lib/api.ts:9-32,208-248`
- NIP-98 `makeAuth` + 10-minute cache — `frontend/src/lib/api.ts:181-206`
- circular-import DI seam — `frontend/src/lib/api.ts:3-7` and `frontend/src/lib/state.ts:48,168-211`
- billing-resource gating + `refetchBilling` — `frontend/src/lib/state.ts:74-103`
- `ensureSessionTenant` + activation ordering — `frontend/src/lib/state.ts:149-165,187-208`
- login rollback — `frontend/src/views/Login.tsx:57-67`
- optimistic mutation — `frontend/src/lib/useRelayToggles.ts:27-36`
