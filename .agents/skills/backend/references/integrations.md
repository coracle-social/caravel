# External integrations (deep detail)

This is the lookup-depth companion to the SKILL.md "external integrations" section: per-leaf behavior, the Stripe idempotency/error/currency details, the NWC per-call pattern, Robot's caches and side effects, the at-rest encryption, and the payment cascade. All of it lives in `stripe.rs`, `wallet.rs`, `bitcoin.rs`, `robot.rs`, `env.rs`, `infra.rs`, and `billing.rs`.

## Stripe leaf

A thin `reqwest` wrapper, no SDK. `get()`/`post()` build a `RequestBuilder` against the Stripe API and attach `.bearer_auth(&stripe_secret_key)` on every call. The `StripeRequest` trait provides `send_ok()` (runs `error_for_status`) and `send_json()`; all methods end in `.send_json()`. `error_for_status` parses Stripe's JSON error envelope into `message [type-or-code] (param: ...)`, falling back to the raw body.

The `Idempotency-Key` is `HMAC-SHA256(stripe_secret_key, parts joined by ':')` with a stable per-operation prefix: `create_customer` keys on `[create_customer, tenant_pubkey]`; the charge (`create_payment_intent`) keys on `[payment_intent, invoice_id, payment_method_id]` — `payment_method_id` is in the key on purpose, so a fall-back to a different card for the same invoice produces a distinct key instead of colliding with (and replaying) the original charge. Reuse `idempotency_key()` with a descriptive prefix for any new mutating call; `get_saved_payment_method` and `create_portal_session` send no idempotency key.

`create_payment_intent` posts `off_session=true`, `confirm=true`, and **requires** `status == "succeeded"`, so the billing cascade falls through via **two distinct paths**: (1) an off-session 3DS/authentication demand is returned by Stripe as an HTTP 402 error, caught earlier by `error_for_status` (do **not** assume `requires_action`/3DS "comes back 2xx" — for off-session confirmed intents Stripe surfaces it as an HTTP error, as the file's own doc comment states); (2) a 2xx response whose status is merely not `"succeeded"` is converted to `Err` by the explicit status check. Don't relax this expecting Stripe to retry off-session. `get_saved_payment_method` returns the **first** card listed (no Stripe-default notion). `create_portal_session` is called directly from the route handler, not via billing. Source: `stripe.rs:1-225` (3DS-as-HTTP-error doc at `104-106`; status check at `135-143`; `error_for_status` at `194-227`), `routes/tenants.rs:263-280`.

## Wallet (NWC) leaf

A parsed `NostrWalletConnectURI` with a short-lived **new-then-shutdown-per-call** pattern: every method does `NWC::new(...)`, awaits the op, then `nwc.shutdown()`. Nothing is pooled across awaits. `is_settled` treats an invoice as settled if `state == Settled` *or* `settled_at.is_some()`; `make_invoice` takes msats + description + expiry and returns a bolt11; `pay_invoice` takes a bolt11. Source: `wallet.rs:7-61`.

## Bitcoin (Coinbase) leaf

Converts fiat-minor to msats: it fetches the Coinbase spot price for `BTC-<CURRENCY>`, divides minor units by `10^exponent`, then `/ price * 1e11`, rounded to `u64`. `currency_minor_exponent` encodes Stripe's currency table (0 decimals for the zero-decimal currencies, 3 for BHD/JOD/KWD/OMR/TND, 2 for any other 3-letter alpha code) and must stay aligned with what Stripe expects. Trap: the Coinbase client is built with **no timeout** (unlike the 5s zooid and robot clients), so a hung response can stall `fiat_to_msats` indefinitely. The Stripe charge currency is hardcoded `"usd"` in billing despite both modules supporting arbitrary currencies, so a non-USD invoice would be charged as a USD-minor amount. Source: `bitcoin.rs:5-50`, `billing.rs:406-416`.

## Robot leaf

The service's Nostr identity, built on `env::get().keys`. `Robot::new()` does no I/O. Announcing the identity is a separate call, `publish_identity().await` (kind 0 metadata, kind 10002 outbox, kind 10050 messaging relays), which `main` makes at startup and which propagates relay send errors. Do not move the publish into `new`, because the e2e harness builds a `Billing` without touching Nostr. `send_dm` discovers the recipient's relays (kind 10002, then kind 10050) and then NIP-17 `send_private_msg`s; empty relay lists are an error. `fetch_nostr_name` swallows errors via `.ok()` and returns `Option`, so a relay outage looks identical to "no name set" (callers fall back to the first 8 chars of the pubkey). Relay-list caches are positive-only with a 5-minute TTL and cache even an empty `Vec`, so a recipient who just published 10002/10050 keeps failing `send_dm` for up to 5 minutes. Source: `robot.rs:11-211`.

## At-rest encryption

`env.encrypt`/`decrypt` are NIP-44 v2 **self-encryption** — the robot's own secret and public key, sender equals recipient — so they provide at-rest confidentiality for the service, not a DM the tenant can read. A tenant's `nwc_url` is encrypted at the route on write and decrypted only at point of use in billing. Outbound zooid auth is NIP-98 via `env.make_auth`, the sole zooid auth mechanism. Source: `env.rs:86-107`, `routes/tenants.rs:130-137`, `billing.rs:381`.

## The payment cascade

`attempt_payment` (the cascade orchestrator, defined in `billing.rs`) runs the cascade in order: (1) NWC auto-pay (decrypt `nwc_url` → per-call `Wallet`), (2) out-of-band lightning `is_settled` via the robot wallet, (3) Stripe card on file, (4) manual DM link. A failing NWC or Stripe attempt records its error on the tenant but never aborts the cascade, and the first success returns `Ok`. Lightning pricing flows through `bitcoin::fiat_to_msats(amount, "usd")` in `ensure_bolt11_for_invoice`, which mints a 3600s bolt11. Note `billing.rs` is *not* the only place integrations are used: route handlers call Stripe and the robot directly too (e.g. `create_tenant` calls `api.robot.fetch_nostr_name` and `api.stripe.create_customer`; `create_stripe_session` calls `api.stripe.create_portal_session`). And a handler may invoke **more than one** billing method — `reconcile_tenant` calls both `sync_stripe_customer` and `reconcile_subscription`, `reconcile_invoice` calls both `ensure_bolt11_for_invoice` and `attempt_payment` — and those public billing methods are themselves orchestrators that fan out internally, not leaf methods. Source: `billing.rs:29-33,326-377,400-502`, `routes/tenants.rs:84-94,182-190`, `routes/invoices.rs:54-62`.

## Per-integration error-string convention

Error-string quality varies, so prefer actionable strings when adding calls. Stripe builds `message [code] (param)` (actionable); zooid builds `method path returned status: body` (actionable). But the leaves are not uniform: NWC's `make_invoice`/`is_settled` add `anyhow!` context (`failed to create invoice: {e}` / `failed to lookup invoice: {e}`), while `pay_invoice` passes the raw error through unchanged (`anyhow!("{e}")`); and Coinbase only wraps the price-*parse* failure (`invalid BTC spot quote for {currency}: {e}`) — a non-2xx Coinbase API response is surfaced as a bare `reqwest` `error_for_status()` error with no added context. (NWC lives in `wallet.rs`, not the files cited below.) Source: `stripe.rs:191-225`, `infra.rs:289-293`, `bitcoin.rs:24-33`, `wallet.rs:39,47,59`.

## Sources

- Stripe leaf — `backend/src/stripe.rs:1-225`, `backend/src/routes/tenants.rs:263-280`
- Wallet (NWC) leaf — `backend/src/wallet.rs:7-61`
- bitcoin leaf — `backend/src/bitcoin.rs:5-50`, `backend/src/billing.rs:406-416`
- Robot leaf — `backend/src/robot.rs:11-211`
- at-rest encryption — `backend/src/env.rs:86-107`, `backend/src/routes/tenants.rs:130-137`, `backend/src/billing.rs:381`
- payment cascade — `backend/src/billing.rs:29-33,326-377,400-502`
- error-string convention — `backend/src/stripe.rs:191-225`, `backend/src/infra.rs:289-293`, `backend/src/bitcoin.rs:24-33`, `backend/src/wallet.rs:39,47,59`
