# `pub async fn create_pool() -> Result<SqlitePool>`

Creates and returns a sqlite connection pool.

Notes:

- Database table names are singular: `activity`, `tenant`, `relay`

Steps:

- Reads `DATABASE_URL` from environment
- Ensures that any directories referred to in `DATABASE_URL` exist
- Initializes the sqlx pool
- Runs migrations found in the `migrations` directory
