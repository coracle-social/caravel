# `async fn main() -> Result<()>`

- Configures logging
- Creates instances of `Repo`, `Robot`, `Billing`, `Api`, and `Infra`
- Spawns `infra.start`
- Get an axum router from `api.router`
- Adds CORS middleware based on `origins`
- Calls `axum::serve` with a listener
