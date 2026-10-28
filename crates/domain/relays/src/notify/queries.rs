//! Reading notifications.

use sqlx::PgExecutor;

use super::Notification;
use caravel_util::error::Result;

/// A pubkey's unexpired message on a relay.
pub async fn get_notification(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    pubkey: &str,
    now: i64,
) -> Result<Option<Notification>> {
    Ok(sqlx::query_as::<_, Notification>(
        "SELECT pubkey, message, created_at, expires_at FROM notification
         WHERE relay_id = $1 AND pubkey = $2 AND (expires_at IS NULL OR expires_at > $3)",
    )
    .bind(relay_id)
    .bind(pubkey)
    .bind(now)
    .fetch_optional(db)
    .await?)
}
