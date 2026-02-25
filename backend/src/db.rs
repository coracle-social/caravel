use anyhow::Result;
use sqlx::{migrate::Migrator, sqlite::SqlitePoolOptions, SqlitePool};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

pub async fn init_pool(database_url: &str) -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;

    MIGRATOR.run(&pool).await?;

    Ok(pool)
}
