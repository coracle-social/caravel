# `pub struct Infra`

Infra is a service which listens for activity and synchronizes relay updates to a remote zooid instance via `api_url`.

Members:

- `api_url: String` - the URL of the zooid instance to be managed, from `ZOOID_API_URL`
- `query: Query`
- `command: Command`

## `pub fn new(query: Query, command: Command) -> Self`

- Reads environment and populates members

## `pub async fn start(self)`

- Subscribes to `command.notify`
- Loops on `rx.recv()`, calling `handle_activity` for each received `Activity`.

## `async fn handle_activity(&self, activity: &Activity)`

- For `create_relay`, `update_relay`, or `deactivate_relay` activity, calls `sync_and_report`.
- All other activity types are ignored (e.g. `fail_relay_sync`, `complete_relay_sync`).

## `async fn sync_and_report(&self, relay: &Relay, is_new: bool)`

- Calls `sync_relay` and on success calls `command.complete_relay_sync`.
- On failure calls `command.fail_relay_sync`.

## `async fn sync_relay(&self, relay: &Relay, is_new: bool)`

- If `is_new`, sends `POST /relay/:id` to create the relay in zooid.
- Otherwise, sends `PUT /relay/:id` to update it.
- Passes full relay configuration in the body including host, schema, secret, inactive flag, info, policy, groups, management, blossom, livekit, push, and roles.
