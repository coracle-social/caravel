//! Reading the call meter.

use sqlx::PgExecutor;

use caravel_util::error::Result;

/// A relay's lifetime call clock: every participant's time in every room. It only
/// climbs, so billing a period takes how far it moved.
pub async fn participant_seconds(db: impl PgExecutor<'_>, relay_id: &str) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(seconds), 0)::bigint FROM livekit_session WHERE relay_id = $1",
    )
    .bind(relay_id)
    .fetch_one(db)
    .await?)
}
