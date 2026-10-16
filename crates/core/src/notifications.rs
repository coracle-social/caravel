//! Notifications: one message per pubkey per relay, which the relay sends as a
//! `NOTIFY` to every connection authenticated as that pubkey, now and whenever it
//! authenticates again, until the message is replaced, cleared or expires.

use serde::{Deserialize, Serialize};
use sqlx::PgExecutor;

use crate::Core;
use crate::bus::Change;
use crate::error::{Error, Result};

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Notification {
    pub pubkey: String,
    pub message: String,
    pub created_at: i64,
    /// When the message stops being sent, if ever.
    pub expires_at: Option<i64>,
}

/// Replace a pubkey's message on a relay, and send it to them now if they are
/// connected.
pub async fn notify(
    core: &Core,
    relay_id: &str,
    pubkey: &str,
    message: &str,
    expires_at: Option<i64>,
) -> Result<()> {
    if message.is_empty() {
        return Err(Error::invalid("message-required", "message is required"));
    }

    sqlx::query(
        "INSERT INTO notification (relay_id, pubkey, message, created_at, expires_at) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (relay_id, pubkey) DO UPDATE
           SET message = EXCLUDED.message, created_at = EXCLUDED.created_at, expires_at = EXCLUDED.expires_at",
    )
    .bind(relay_id)
    .bind(pubkey)
    .bind(message)
    .bind(core.clock.now())
    .bind(expires_at)
    .execute(core.db.pool())
    .await?;

    core.bus.publish(Change::Notified {
        relay_id: relay_id.to_string(),
        pubkey: pubkey.to_string(),
        message: message.to_string(),
    });
    Ok(())
}

/// Drop a pubkey's message. Connections already sent it aren't told.
pub async fn clear(db: impl PgExecutor<'_>, relay_id: &str, pubkey: &str) -> Result<()> {
    sqlx::query("DELETE FROM notification WHERE relay_id = $1 AND pubkey = $2")
        .bind(relay_id)
        .bind(pubkey)
        .execute(db)
        .await?;
    Ok(())
}

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
