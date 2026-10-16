//! Postgres access shared by every layer above it: the connection pool, the
//! schema migrations, and transactions that retry on serialization failures.

use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::{PgConnection, PgPool};

#[cfg(feature = "testing")]
pub mod testing;

/// A pool of Postgres connections. The app opens one for interactive requests and
/// one for background work, so workers can never starve the hot path.
#[derive(Clone, Debug)]
pub struct Db {
    pool: PgPool,
}

/// An error that may be worth retrying, because Postgres aborted the transaction
/// to resolve a conflict with another one rather than because it was wrong.
pub trait Retryable {
    fn is_retryable(&self) -> bool;
}

impl Retryable for sqlx::Error {
    fn is_retryable(&self) -> bool {
        // 40001 serialization_failure, 40P01 deadlock_detected
        matches!(sqlstate(self).as_deref(), Some("40001" | "40P01"))
    }
}

impl Db {
    /// Connect a pool of at most `max_connections` to `url`.
    pub async fn connect(url: &str, max_connections: u32) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .acquire_timeout(Duration::from_secs(10))
            .connect(url)
            .await?;

        Ok(Self { pool })
    }

    /// Bring the schema up to date.
    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations").run(&self.pool).await
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Run `f` in a transaction, committing if it succeeds. A transaction Postgres
    /// aborts over a conflict with a concurrent one is run again from the start on
    /// a fresh clone of `f`, so `f` must not have effects outside the database.
    pub async fn with_tx<T, E, F>(&self, f: F) -> Result<T, E>
    where
        F: AsyncFnOnce(&mut PgConnection) -> Result<T, E> + Clone,
        E: From<sqlx::Error> + Retryable,
    {
        let mut attempt = 0;

        loop {
            let mut tx = self.pool.begin().await?;
            let result = match f.clone()(&mut tx).await {
                Ok(value) => tx.commit().await.map(|_| value).map_err(E::from),
                Err(error) => Err(error),
            };

            match result {
                Err(error) if attempt < 4 && error.is_retryable() => {
                    attempt += 1;
                    tracing::debug!(attempt, "retrying a conflicting transaction");
                    tokio::time::sleep(Duration::from_millis(10 << attempt)).await;
                }
                result => return result,
            }
        }
    }
}

/// The SQLSTATE code of a database error.
fn sqlstate(error: &sqlx::Error) -> Option<String> {
    match error {
        sqlx::Error::Database(error) => error.code().map(|code| code.into_owned()),
        _ => None,
    }
}

/// The name of the unique constraint `error` violated, if that is what it is.
pub fn unique_violation(error: &sqlx::Error) -> Option<&str> {
    match error {
        sqlx::Error::Database(error) if error.is_unique_violation() => error.constraint(),
        _ => None,
    }
}
