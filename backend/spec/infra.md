# `pub struct Infra`

Infra is a background worker that listens for activity and synchronizes relay configuration to a remote zooid instance.

Members:

- `env: Env` - configuration; supplies `zooid_api_url`, `relay_domain`, the `BLOSSOM_S3_*` and `LIVEKIT_*` settings, and the robot key used to sign requests
- `query: Query`
- `command: Command`

## `pub fn new(query: Query, command: Command, env: &Env) -> Self`

- Stores `query`, `command`, and a clone of `env`

## `pub async fn start(self)`

- Subscribes to `command.notify`
- Runs `reconcile_relay_state("startup")` before entering the loop
- Loops on `rx.recv()`, calling `handle_activity` for each `Activity`
- On `Lagged`, runs `reconcile_relay_state("lagged")`; on `Closed`, exits

## `async fn handle_activity(&self, activity: &Activity)`

- Ignores anything that isn't a `relay` resource with activity type `create_relay`, `update_relay`, `activate_relay`, `deactivate_relay`, or `fail_relay_sync`
- For `fail_relay_sync`, schedules a delayed retry via `schedule_relay_sync_retry`
- Otherwise resolves the relay (skip if gone) and calls `sync_relay`

## `async fn reconcile_relay_state(&self, source: &str)`

- Lists relays still pending sync (`query.list_relays_pending_sync`)
- For each: `sync_relay` immediately if its `sync_error` is empty, otherwise `schedule_relay_sync_retry`

## `async fn schedule_relay_sync_retry(&self, relay_id: &str, source: &str)`

- Counts the relay's consecutive trailing `fail_relay_sync` activities to derive the attempt number
- Computes an exponential backoff (base 30s, doubling, capped at 15 minutes); gives up after `RELAY_SYNC_RETRY_MAX_ATTEMPTS` (6) to avoid infinite retry loops
- Spawns a task that sleeps for the delay, then re-reads the relay and `sync_relay`s it (no-op if the relay is gone)

## `async fn sync_relay(&self, relay: &Relay)`

- Calls `try_sync_relay`; on success `command.complete_relay_sync`, on failure `command.fail_relay_sync` with the error

## `async fn try_sync_relay(&self, relay: &Relay)`

- A relay is "new" only if it has never completed a sync (`synced != 1` and no `complete_relay_sync` activity exists). New relays are created with `POST /relay/:id`; existing relays are updated with `PATCH /relay/:id`.
- A freshly generated `secret` is included only for creation (`POST`), so updates don't rotate relay identity and we never store the secret.
- The body carries relay configuration: `host` (= `subdomain.relay_domain`), `schema`, `inactive` (true when status is `inactive` or `delinquent`), `info` (name/icon/description/pubkey), `policy`, `groups`, `management`, `blossom`, `livekit`, `push`, and hard-coded `roles`.
- When `blossom_enabled`, the blossom section uses `adapter: "s3"` with the `BLOSSOM_S3_*` settings and `s3.key_prefix` set to the relay's `schema`; otherwise it sends `{ "enabled": false }`.
- When `livekit_enabled`, the livekit section carries the `LIVEKIT_*` settings; otherwise `{ "enabled": false }`.

## `pub async fn list_relay_members(&self, relay_id: &str) -> Result<Vec<String>>`

- `GET /relay/:id/members` from zooid; returns the `members` array

## `async fn request(&self, method, path, body)`

- Sends an authenticated request to the zooid API at `path` (relative to `env.zooid_api_url`), with a 5s timeout
- Authenticates each request with a NIP-98 header via `env.make_auth`
- Returns the response on 2xx; bails with the status and body text otherwise
