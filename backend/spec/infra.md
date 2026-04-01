# `pub struct Infra`

Infra is a service which polls the database and synchronizes updates to relays to a remote zooid instance via `api_url`.

Members:

- `api_url: String` - the URL of the zooid instance to be managed, from `ZOOID_API_URL`
- `query: Query`
- `command: Command`

## `pub fn new(query: Query, command: Command) -> Self`

- Reads environment and populates members

## `pub async fn start(self)`

- Initializes `last_activity_at` from `query.max_activity_at()` so historical activities are not replayed on restart.
- Calls `self.tick` in a loop every 10 seconds.

## `pub async fn tick(self)`

Iterates over `query.list_activity` since last run and does the following:

- For `create_relay`, `update_relay`, or `deactivate_relay` activity, sync the relay to zooid.
  - Uses `relay.synced` to decide POST vs PUT (not the activity type), so already-synced relays always use PUT even on restart.
  - On success, calls `command.mark_relay_synced` to set `synced = 1`, `status = 'active'`, and clear `sync_error`.
  - On failure, calls `command.fail_relay_sync`.
- All other activity types are ignored (e.g. `fail_relay_sync` must not trigger another sync).

## `async fn sync_relay(&self, relay: &Relay, is_new: bool)`

- If `is_new`, sends `POST /relay/:id` to create the relay in zooid.
- Otherwise, sends `PUT /relay/:id` to update it.
- Passes full relay configuration in the body including host, schema, secret, inactive flag, info, policy, groups, management, blossom, livekit, push, and roles.
