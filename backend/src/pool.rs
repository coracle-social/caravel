use std::path::Path;
use std::str::FromStr;

use anyhow::Result;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

pub async fn create_pool() -> Result<SqlitePool> {
    let raw_database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| format!("sqlite://{}/data/caravel.db", env!("CARGO_MANIFEST_DIR")));
    let database_url = normalize_sqlite_url(&raw_database_url);

    if let Some(path) = database_url.strip_prefix("sqlite://")
        && !path.is_empty()
        && path != ":memory:"
        && let Some(parent) = Path::new(path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let connect_options =
        SqliteConnectOptions::from_str(&database_url)?.create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(connect_options)
        .await?;

    sqlx::query("PRAGMA journal_mode = WAL;")
        .execute(&pool)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}

fn normalize_sqlite_url(url: &str) -> String {
    let Some(path) = url.strip_prefix("sqlite://") else {
        return url.to_string();
    };

    if path.is_empty() || path == ":memory:" || Path::new(path).is_absolute() {
        return url.to_string();
    }

    format!("sqlite://{}/{}", env!("CARGO_MANIFEST_DIR"), path)
}
