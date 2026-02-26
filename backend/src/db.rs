use anyhow::{anyhow, Result};
use sqlx::{migrate::Migrator, sqlite::SqliteConnectOptions, sqlite::SqlitePoolOptions, SqlitePool};
use std::str::FromStr;
use std::fs;
use std::path::Path;

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
    const PREFIX: &str = "sqlite://";
    if !database_url.starts_with(PREFIX) {
        return Ok(());
    }

    let path = &database_url[PREFIX.len()..];
    if path.is_empty() || path == ":memory:" {
        return Ok(());
    }

    let path = if let Some(stripped) = path.strip_prefix('/') {
        format!("/{}", stripped)
    } else {
        path.to_string()
    };

    let parent = Path::new(&path)
        .parent()
        .ok_or_else(|| anyhow!("invalid sqlite path"))?;
    fs::create_dir_all(parent)?;
    Ok(())
}
