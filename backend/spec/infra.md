# `pub struct Infra`

Infra is a service which listens for activity and synchronizes relay updates to a remote zooid instance via `api_url`.

Members:

- `api_url: String` - the URL of the zooid instance to be managed, from `ZOOID_API_URL`
- `blossom_s3: Option<BlossomS3Sync>` - shared Blossom S3 settings from `BLOSSOM_S3_*` when region, bucket, access key, and secret are all non-empty after trim
- `query: Query`
- `command: Command`

## `pub fn new(query: Query, command: Command) -> Self`

- Reads environment and populates members

## `pub async fn start(self)`

- Subscribes to `command.notify`
- On startup, schedules delayed sync retries for relays whose `sync_error` is non-empty.
- Loops on `rx.recv()`, calling `handle_activity` for each received `Activity`.

## `async fn handle_activity(&self, activity: &Activity)`

- For `create_relay`, `update_relay`, `activate_relay`, or `deactivate_relay` activity, calls `sync_and_report` immediately.
- For `fail_relay_sync`, schedules a delayed retry using exponential backoff based on consecutive failures for the relay.
- Retry scheduling stops after the configured max attempts to avoid infinite retry loops.
- Other activity types are ignored (e.g. `complete_relay_sync`).

## `async fn sync_and_report(&self, relay: &Relay, is_new: bool)`

- Calls `sync_relay` and on success calls `command.complete_relay_sync`.
- On failure calls `command.fail_relay_sync`.

## `async fn sync_relay(&self, relay: &Relay, is_new: bool)`

- If `is_new`, sends `POST /relay/:id` to create the relay in zooid.
- Otherwise, sends `PATCH /relay/:id` to update it.
- Includes `secret` only for relay creation (`POST`) so updates do not rotate relay identity.
- Passes relay configuration in the body including host, schema, inactive flag, info, policy, groups, management, blossom, livekit, push, and roles.
- When `blossom_s3` is configured and the relay has blossom enabled, the blossom section includes `adapter: "s3"`, S3 fields from the environment, and `s3.key_prefix` set to the relay's `schema`. Otherwise blossom omits S3 (zooid defaults to local storage) or sends `{ "enabled": false }` when blossom is disabled.
