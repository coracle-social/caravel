use std::path::Path;
use std::str::FromStr;
use std::sync::OnceLock;

use anyhow::Result;
use sqlx::{
    ConnectOptions, Connection, Sqlite, SqlitePool, Transaction,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tokio::sync::broadcast;

use crate::env;
use crate::models::Activity;

/// Process-wide connection pool. Set once at startup via [`init`]; read
/// everywhere else via [`pool`], so command/query stay free functions instead of
/// threading a handle through every service.
static POOL: OnceLock<SqlitePool> = OnceLock::new();

/// Process-wide activity broadcast. Mutations record an [`Activity`] and call
/// [`publish`] after their transaction commits; reactors (billing, infra)
/// [`subscribe`] to react to durable changes.
static NOTIFY: OnceLock<broadcast::Sender<Activity>> = OnceLock::new();

/// Create the connection pool from `env`, run migrations, and store it as the
/// process-wide global. Panics if called more than once.
pub async fn init() -> Result<()> {
    let pool = create_pool(&env::get().database_url).await?;
    POOL.set(pool).expect("pool already initialized");

    let (notify, _) = broadcast::channel(64);
    NOTIFY.set(notify).expect("notify already initialized");

    Ok(())
}

/// The global pool. Panics if [`init`] hasn't run yet.
pub fn pool() -> &'static SqlitePool {
    POOL.get().expect("pool not initialized")
}

/// Subscribe to the activity stream. Panics if [`init`] hasn't run yet.
pub fn subscribe() -> broadcast::Receiver<Activity> {
    NOTIFY.get().expect("notify not initialized").subscribe()
}

/// Broadcast an activity to subscribers. Called after the writing transaction
/// commits, so reactors only ever observe durable rows. A send with no current
/// subscribers is intentionally ignored.
pub fn publish(activity: Activity) {
    if let Some(notify) = NOTIFY.get() {
        let _ = notify.send(activity);
    }
}

/// Run `f` inside a transaction, commit on success, and roll back (on drop) if
/// it returns an error. Returns whatever `f` produces. Callers compose the
/// transaction-scoped `command`/`query` functions inside `f` to make a
/// multi-step write atomic.
pub async fn with_tx<F, T>(f: F) -> Result<T>
where
    F: AsyncFnOnce(&mut Transaction<'_, Sqlite>) -> Result<T>,
{
    let mut tx = pool().begin().await?;
    let value = f(&mut tx).await?;
    tx.commit().await?;
    Ok(value)
}

async fn create_pool(database_url: &str) -> Result<SqlitePool> {
    let database_url = normalize_sqlite_url(database_url);

    if let Some(path) = database_url.strip_prefix("sqlite://")
        && !path.is_empty()
        && path != ":memory:"
        && let Some(parent) = Path::new(path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let connect_options = SqliteConnectOptions::from_str(&database_url)?.create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(connect_options.clone())
        .await?;

    sqlx::query("PRAGMA journal_mode = WAL;")
        .execute(&pool)
        .await?;

    // Migrations get their own connection, with foreign key enforcement off.
    // SQLite's procedure for changing a table's schema — rebuild, copy, drop,
    // rename — requires that, and `PRAGMA foreign_keys` is a no-op inside the
    // transaction sqlx wraps each migration in, so it has to be set at connect
    // time. The pool above keeps enforcement on for everything at runtime.
    let mut migrate_conn = connect_options.foreign_keys(false).connect().await?;

    sqlx::migrate!("./migrations")
        .run(&mut migrate_conn)
        .await?;

    migrate_conn.close().await?;

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
