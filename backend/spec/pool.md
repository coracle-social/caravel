# `pub async fn create_pool(database_url: &str) -> Result<SqlitePool>`

Creates and returns a sqlite connection pool.

Notes:

- Database table names are singular: `activity`, `tenant`, `relay`, `lightning_invoice`

Steps:

- Normalizes `database_url`: a relative `sqlite://` path is resolved under the crate manifest directory (`CARGO_MANIFEST_DIR`); absolute paths and `:memory:` are left as-is
- Ensures any directory referred to in the (normalized) URL exists
- Opens the pool with `create_if_missing` enabled
- Enables WAL journaling (`PRAGMA journal_mode = WAL`)
- Runs migrations found in the `migrations` directory
