# `pub struct Infra`

Infra is a service which polls the database and synchronizes updates to relays to a remote zooid instance via `api_url`.

Members:

- `api_url: String` - the URL of the zooid instance to be managed, from `ZOOID_API_URL`
- `repo: Repo`

## `pub fn new(repo: Repo) -> Self`

- Reads environment and populates members

## `pub async fn start(self)`

Calls `self.tick` in a loop every 10 seconds.

## `pub async fn tick(self)`

Iterates over `repo.list_activity` since last run and does the following:

- For `create_relay` activity, create the relay in zooid via `POST /relay`.
- For `update_relay` or `deactivate_relay` activity, update the relay in zooid via `PUT /relay/:id`.
- All other activity types are ignored (e.g. `fail_relay_sync` must not trigger another sync).
- If unsuccessful, call `repo.fail_relay_sync`.

## `async fn sync_relay(&self, relay: &Relay, is_new: bool)`

- If `is_new`, sends `POST /relay` to create the relay in zooid.
- Otherwise, sends `PUT /relay/:id` to update it.
- Passes full relay configuration in the body including host, schema, secret, inactive flag, info, policy, groups, management, blossom, livekit, push, and roles.
