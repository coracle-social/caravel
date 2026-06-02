# Reactors and relay sync (deep detail)

This is the lookup-depth companion to the SKILL.md "background reactors" section: the relay-sync retry/backoff machinery, the self-feeding failure loop, the zooid POST-vs-PATCH request, and the billing dunning cascade timing. All of it lives in `infra.rs`, `billing.rs`, `db.rs`, and `query.rs`.

## The infra recv loop

`infra::start()` calls `db::subscribe()` once, runs `reconcile_relay_state("startup")` to recover relays left unsynced from a prior run, then loops on `rx.recv().await`, handling all three broadcast outcomes:

- **`Ok(activity)`** → `handle_activity`, which filters to `resource_type == "relay"` *and* an `activity_type` in `{create_relay, update_relay, activate_relay, deactivate_relay, fail_relay_sync}`; everything else is ignored. A `fail_relay_sync` routes to `schedule_relay_sync_retry`; the others load the relay via `query::get_relay` and call `sync_relay`.
- **`Lagged(n)`** → `warn` plus a full `reconcile_relay_state("lagged")` sweep to recover the dropped messages.
- **`Closed`** → break out of the loop, terminating the worker. Because the broadcast `Sender` lives in a `static OnceLock` for the whole process, `Closed` effectively never happens in normal operation — but if it did, `infra::start` returns and is not restarted by `main`, leaving relay provisioning dead until the process restarts.

`reconcile_relay_state` queries `list_relays_pending_sync` (`synced = 0 OR TRIM(sync_error) != ''`), returns early if empty, and otherwise routes blank-error relays to an immediate `sync_relay` and error-carrying ones through backoff. Source: `infra.rs:28-92`, `query.rs:81-83`.

## Backoff

`schedule_relay_sync_retry` counts `consecutive_failures` via `take_while` over `fail_relay_sync` activities at the **head** of the resource history (ordered `created_at DESC`) — any non-failure activity at the head resets the count to 0, which is what lets a recovered relay restart backoff from the base delay. The delay is `BASE(30s) << (attempt - 1)`, capped at `MAX(15min)`; after `MAX_ATTEMPTS(6)` it returns `None`, logs "retries exhausted; awaiting manual intervention", and stops. The retry itself is a fire-and-forget `tokio::spawn` that sleeps the computed delay, re-fetches the relay, and calls `sync_relay` (a missing relay is a silent no-op). Source: `infra.rs:15-17,94-148`, `query.rs:242-249`.

## The self-feeding loop

`sync_relay` never returns an error: on `Ok` it calls `command::complete_relay_sync` (sets `synced = 1`, `sync_error = ''`); on `Err` it calls `command::fail_relay_sync` (sets `synced = 0`, `sync_error = ...`), which publishes a `fail_relay_sync` activity after commit, which re-enters `handle_activity` and re-schedules backoff. The retry chain terminates when **any** of these happen: the sync succeeds (`complete_relay_sync` resets `synced = 1` and breaks the consecutive-failure streak counted by `take_while`, so no further retry is scheduled), the relay no longer exists (`get_relay` returns `None`, a silent no-op), a `get_relay` query errors (logged and stopped), or the consecutive-failure count exceeds `MAX_ATTEMPTS(6)` — after which the relay sits with `synced = 0` and a set `sync_error` until manual intervention or another activity touches it. Note `set_relay_status_tx` and `update_relay` always reset `synced = 0` as a side effect, so a "pure" status flip is never sync-neutral. Source: `infra.rs:57-60,136-146,151-166`, `command.rs:185-273,580-596`.

## The zooid request

`try_sync_relay` assembles the request body inline as a `serde_json::json!`: `host` (subdomain + `relay_domain`), `schema` (`relay.id`), an `inactive` flag, and the `info`/`policy`/`groups`/`management`/`push`/`roles` blocks, plus a conditional blossom S3 block and a livekit block — each gated on the relay's `*_enabled` i64 flag and falling back to `{enabled: false}`.

`is_new` is true **only** when `synced != 1` *and* there is no prior `complete_relay_sync` activity. `is_new` alone decides `POST` (with a freshly generated `Keys::generate` secret inserted into the body) vs `PATCH` (secret omitted). Because `update_relay` resets `synced = 0`, a re-sync after an update would look "new" by the `synced` flag alone — the second condition (no prior `complete_relay_sync`) is what makes it a `PATCH`, so the relay is not re-created and its secret is not clobbered. Caravel never persists the secret, so this check is load-bearing.

All zooid calls go through `request(method, path, body)`: a 5-second `reqwest` client, base from `zooid_api_url` (trailing slash trimmed), NIP-98 `Authorization` via `env.make_auth`, and a non-2xx response is turned into an `anyhow::bail!` carrying the status and body. Source: `infra.rs:168-295`.

## Billing worker timing

`POLL_INTERVAL` is 1 hour, so dunning runs at hour granularity. The DM guards exist specifically so the hourly tick doesn't re-DM on every pass:

- `GRACE_PERIOD_SECS` = 7 days (dunning grace before churn)
- `FRESH_INVOICE_DM_GRACE_SECS` = 24h (hold the manual-payment DM until an open invoice is at least this old, because a fresh invoice is surfaced in-app first)
- `MANUAL_PAYMENT_DM_INTERVAL_SECS` = 12 days (minimum spacing between reminder DMs)

`attempt_payment_using_dm` checks both `invoice.created_at` and `invoice.notified_at` before sending. `reconcile_subscription` clones the tenant and mutates the local copy (billing anchor, churn, payment method), updating the DB via explicit `command` calls, so the synchronous reconcile route re-reads the tenant afterward to reflect the changes. Source: `billing.rs:15-23,46-130,436-449`.

## Sources

- infra recv loop + reconcile — `backend/src/infra.rs:28-92`, `backend/src/query.rs:81-83`
- backoff — `backend/src/infra.rs:15-17,94-148`, `backend/src/query.rs:242-249`
- self-feeding loop — `backend/src/infra.rs:57-60,136-146,151-166`, `backend/src/command.rs:185-273,580-596`
- zooid request + POST-vs-PATCH — `backend/src/infra.rs:168-295`
- billing worker timing — `backend/src/billing.rs:15-23,46-130,436-449`
