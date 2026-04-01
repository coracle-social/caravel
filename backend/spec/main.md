# `async fn main() -> Result<()>`

- Configures logging
- Calls `create_pool` to get a `SqlitePool`, then creates `Query`, `Command`, `Robot`, `Billing`, `Api`, and `Infra`
- Get an axum router from `api.router`
- Adds CORS middleware based on `origins`
- Calls `axum::serve` with a listener
- Spawns `infra.start`
