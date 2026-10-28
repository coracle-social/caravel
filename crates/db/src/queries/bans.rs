//! Reading who and what a relay refuses.

use sqlx::PgExecutor;

use caravel_domain::bans::Ban;
use caravel_util::error::Result;

pub async fn list_banned_pubkeys(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<Ban>> {
    Ok(sqlx::query_as::<_, Ban>(
        "SELECT pubkey AS subject, reason, created_at FROM banned_pubkey
         WHERE relay_id = $1 ORDER BY created_at, pubkey",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

pub async fn list_banned_events(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<Ban>> {
    Ok(sqlx::query_as::<_, Ban>(
        "SELECT event_id AS subject, reason, created_at FROM banned_event
         WHERE relay_id = $1 ORDER BY created_at, event_id",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}
