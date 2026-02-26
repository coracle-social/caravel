use anyhow::Result;
use sqlx::{migrate::Migrator, sqlite::SqliteConnectOptions, sqlite::SqlitePoolOptions, SqlitePool};
use std::fs;
use std::path::Path;
use std::str::FromStr;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

pub async fn init_pool(database_url: &str) -> Result<SqlitePool> {
    ensure_sqlite_directory(database_url)?;
    let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    sqlx::query("PRAGMA journal_mode = WAL;")
        .execute(&pool)
        .await?;

    MIGRATOR.run(&pool).await?;

    Ok(pool)
}

fn ensure_sqlite_directory(database_url: &str) -> Result<()> {
    let Some(path) = sqlite_path(database_url) else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn sqlite_path(database_url: &str) -> Option<&Path> {
    const PREFIX: &str = "sqlite://";
    let path = database_url.strip_prefix(PREFIX)?;
    if path.is_empty() || path == ":memory:" {
        return None;
    }
    Some(Path::new(path))
}
