# `async fn main() -> Result<()>`

- Loads `.env` (via `dotenvy`) and configures tracing/logging from the default env filter
- Calls `Env::load()` to read and validate all configuration
- Calls `create_pool(&env.database_url)` to get a `SqlitePool`
- Constructs the services, passing `&env` where needed:
  - `Robot::new(&env).await` (publishes the robot's nostr identity)
  - `Stripe::new(&env)`
  - `Query::new(pool.clone(), &env)`
  - `Command::new(pool)`
  - `Billing::new(query.clone(), command.clone(), &env)`
  - `Infra::new(query.clone(), command.clone(), &env)`
  - `Api::new(query, command, billing.clone(), stripe, robot, infra.clone(), &env)`
- Builds a CORS layer restricted to the parsed `env.server_allow_origins`
- Gets the axum router from `api.router()` and applies the CORS layer
- Spawns `infra.start()` and `billing.start()` as background tasks
- Binds a `TcpListener` on `env.server_host:env.server_port` and calls `axum::serve`
