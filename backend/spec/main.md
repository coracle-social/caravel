# `async fn main() -> Result<()>`

- Configures logging
- Creates instances of `Repo`, `Robot`, `Billing`, `Api`, and `Infra`
- Spawns `billing.start()`
- Spawns `infra.start()`
- Calls `api.serve()`
