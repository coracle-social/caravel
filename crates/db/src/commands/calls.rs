//! Running the call meter.

use sqlx::PgExecutor;

use caravel_domain::error::Result;

/// Credit a participant's time in a call, from when they joined until they left,
/// unless LiveKit didn't say when they joined.
pub async fn record_departure(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    room_id: &str,
    sid: &str,
    joined_at: i64,
    left_at: i64,
) -> Result<()> {
    if joined_at <= 0 || left_at <= joined_at {
        return Ok(());
    }
    record_session(db, relay_id, sid, room_id, left_at - joined_at, left_at).await
}

/// Credit one participant's closed session. A redelivered webhook for the same
/// session changes nothing.
pub async fn record_session(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    sid: &str,
    room_id: &str,
    seconds: i64,
    ended_at: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO livekit_session (relay_id, sid, room_id, seconds, ended_at) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (relay_id, sid) DO NOTHING",
    )
    .bind(relay_id)
    .bind(sid)
    .bind(room_id)
    .bind(seconds)
    .bind(ended_at)
    .execute(db)
    .await?;
    Ok(())
}
