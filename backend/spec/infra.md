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

- For any `relay_created|relay_updated` activity, sync relay config to zooid.
- For any `relay_deactivated` activity, sync relay config to zooid.
- If unsuccessful, call `repo.fail_relay_sync`.
