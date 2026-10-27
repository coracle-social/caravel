//! Starting and ending sessions.

use sqlx::PgExecutor;

use crate::crypto::{random_token, sha256_hex};
use crate::error::Result;
use crate::models::sessions::{SESSION_TTL, Session};

/// Start a session for `pubkey`, returning the bearer token that stands for it.
/// Only the token's hash is stored.
pub async fn create_session(
    db: impl PgExecutor<'_>,
    pubkey: &str,
    now: i64,
) -> Result<(String, Session)> {
    let token = random_token();
    let session = sqlx::query_as::<_, Session>(
        "INSERT INTO session (token_hash, pubkey, created_at, expires_at, last_used_at)
         VALUES ($1, $2, $3, $4, $3)
         RETURNING pubkey, created_at, expires_at, last_used_at",
    )
    .bind(sha256_hex(token.as_bytes()))
    .bind(pubkey)
    .bind(now)
    .bind(now + SESSION_TTL)
    .fetch_one(db)
    .await?;

    Ok((token, session))
}

pub async fn delete_session(db: impl PgExecutor<'_>, token: &str) -> Result<()> {
    sqlx::query("DELETE FROM session WHERE token_hash = $1")
        .bind(sha256_hex(token.as_bytes()))
        .execute(db)
        .await?;
    Ok(())
}

/// End every session `pubkey` holds.
pub async fn revoke_sessions(db: impl PgExecutor<'_>, pubkey: &str) -> Result<u64> {
    let result = sqlx::query("DELETE FROM session WHERE pubkey = $1")
        .bind(pubkey)
        .execute(db)
        .await?;
    Ok(result.rows_affected())
}

pub async fn delete_expired_sessions(db: impl PgExecutor<'_>, now: i64) -> Result<u64> {
    let result = sqlx::query("DELETE FROM session WHERE expires_at <= $1")
        .bind(now)
        .execute(db)
        .await?;
    Ok(result.rows_affected())
}

/// Record that a session was used at `now`.
pub async fn touch_session(db: impl PgExecutor<'_>, token: &str, now: i64) -> Result<()> {
    sqlx::query("UPDATE session SET last_used_at = $1 WHERE token_hash = $2")
        .bind(now)
        .bind(sha256_hex(token.as_bytes()))
        .execute(db)
        .await?;
    Ok(())
}
