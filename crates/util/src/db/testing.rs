//! A database per test binary, created fresh from `TEST_DATABASE_URL` the first
//! time a test asks for it, so tests in one binary share a schema and tests in
//! different binaries never see each other's rows.

use sqlx::{Connection, PgConnection};
use tokio::sync::OnceCell;

use super::Db;

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
