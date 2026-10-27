//! Reading sessions.

use sqlx::PgExecutor;

use crate::crypto::sha256_hex;
use crate::error::Result;
use crate::models::sessions::Session;

/// The live session a bearer token stands for.
pub async fn get_session(
    db: impl PgExecutor<'_>,
    token: &str,
    now: i64,
) -> Result<Option<Session>> {
    Ok(sqlx::query_as::<_, Session>(
        "SELECT pubkey, created_at, expires_at, last_used_at FROM session
         WHERE token_hash = $1 AND expires_at > $2",
    )
    .bind(sha256_hex(token.as_bytes()))
    .bind(now)
    .fetch_optional(db)
    .await?)
}
