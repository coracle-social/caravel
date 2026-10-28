//! Postgres access shared by every layer above it: the connection pool, the
//! schema migrations, and transactions that retry on serialization failures.

use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::{PgConnection, PgPool};

/// A connection inside a transaction, as `with_tx` hands it to its work.
pub type Connection = PgConnection;

/// A pool of connections, as `Db::pool` returns.
pub type Pool = PgPool;

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

#[cfg(feature = "testing")]
pub mod testing {
    //! A database per test binary, created fresh from `TEST_DATABASE_URL` the first
    //! time a test asks for it, so tests in one binary share a schema and tests in
    //! different binaries never see each other's rows.

    use super::Db;
    use sqlx::{Connection, PgConnection};
    use tokio::sync::OnceCell;

    const DEFAULT_URL: &str = "postgres://caravel:caravel@127.0.0.1:55433/postgres";

    static URL: OnceCell<String> = OnceCell::const_new();

    /// A pool on this test binary's database, migrated and ready.
    pub async fn connect() -> Db {
        let url = URL.get_or_init(create_database).await;

        Db::connect(url, 4)
            .await
            .expect("failed to connect to the test database")
    }

    async fn create_database() -> String {
        #[allow(clippy::disallowed_methods)]
        let admin_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| DEFAULT_URL.into());
        let name = database_name();

        let mut admin = PgConnection::connect(&admin_url)
            .await
            .unwrap_or_else(|error| panic!("can't reach {admin_url} (run `just pg`): {error}"));

        sqlx::query(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
            .execute(&mut admin)
            .await
            .expect("failed to drop the old test database");
        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(&mut admin)
            .await
            .expect("failed to create the test database");
        admin.close().await.ok();

        let (base, _) = admin_url
            .rsplit_once('/')
            .expect("TEST_DATABASE_URL has no database");
        let query = admin_url
            .split_once('?')
            .map(|(_, query)| format!("?{query}"));
        let url = format!("{base}/{name}{}", query.unwrap_or_default());

        let db = Db::connect(&url, 1)
            .await
            .expect("failed to connect to the test database");
        db.migrate()
            .await
            .expect("failed to migrate the test database");
        db.pool().close().await;

        url
    }

    /// `caravel_test_<binary>_<hash>`, from the test executable's name, whose hash
    /// cargo derives from its crate so same-named binaries in two crates differ.
    fn database_name() -> String {
        let exe = std::env::current_exe().expect("no current executable");
        let stem = exe
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("tests");
        let binary: String = stem
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();

        format!("caravel_test_{binary}")
    }
}
