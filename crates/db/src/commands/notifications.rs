//! Replacing and clearing notifications.

use sqlx::PgExecutor;

use caravel_domain::error::Result;

/// Drop a pubkey's message. Connections already sent it aren't told.
pub async fn clear(db: impl PgExecutor<'_>, relay_id: &str, pubkey: &str) -> Result<()> {
    sqlx::query("DELETE FROM notification WHERE relay_id = $1 AND pubkey = $2")
        .bind(relay_id)
        .bind(pubkey)
        .execute(db)
        .await?;
    Ok(())
}

/// Replace a pubkey's message on a relay.
pub async fn set_notification(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    pubkey: &str,
    message: &str,
    expires_at: Option<i64>,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO notification (relay_id, pubkey, message, created_at, expires_at) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (relay_id, pubkey) DO UPDATE
           SET message = EXCLUDED.message, created_at = EXCLUDED.created_at, expires_at = EXCLUDED.expires_at",
    )
    .bind(relay_id)
    .bind(pubkey)
    .bind(message)
    .bind(now)
    .bind(expires_at)
    .execute(db)
    .await?;
    Ok(())
}
