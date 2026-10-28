//! Reading a relay's activity log.

use sqlx::PgExecutor;

use crate::relays::Activity;
use caravel_util::error::Result;

/// A tenant's billable activity that billing hasn't reconciled, oldest first so
/// line items and proration apply in the order things happened. Marking a relay
/// delinquent isn't billable: it is billing's own doing, not the tenant's.
pub async fn list_billable_activity_for_tenant(
    db: impl PgExecutor<'_>,
    tenant_pubkey: &str,
) -> Result<Vec<Activity>> {
    Ok(sqlx::query_as::<_, Activity>(
        "SELECT * FROM activity
         WHERE tenant_pubkey = $1 AND billed_at IS NULL
           AND activity_type IN (
               'create_relay', 'update_relay', 'activate_relay', 'deactivate_relay',
               'purge_relay', 'unmark_relay_delinquent'
           )
         ORDER BY created_at, seq",
    )
    .bind(tenant_pubkey)
    .fetch_all(db)
    .await?)
}

/// The relay's latest activity strictly before `before`, or `None` if it didn't
/// exist then. Strict, so a relay created exactly at a period boundary isn't
/// counted active there: its creation charge covers that period.
pub async fn get_latest_relay_activity_before(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    before: i64,
) -> Result<Option<Activity>> {
    Ok(sqlx::query_as::<_, Activity>(
        "SELECT * FROM activity
         WHERE relay_id = $1 AND created_at < $2
         ORDER BY created_at DESC, seq DESC
         LIMIT 1",
    )
    .bind(relay_id)
    .bind(before)
    .fetch_optional(db)
    .await?)
}

/// The relay's activity immediately preceding `activity`, which holds the state a
/// change moved the relay from.
pub async fn get_previous_relay_activity(
    db: impl PgExecutor<'_>,
    activity: &Activity,
) -> Result<Option<Activity>> {
    Ok(sqlx::query_as::<_, Activity>(
        "SELECT * FROM activity
         WHERE relay_id = $1
           AND (created_at, seq) < (SELECT created_at, seq FROM activity WHERE id = $2)
         ORDER BY created_at DESC, seq DESC
         LIMIT 1",
    )
    .bind(&activity.relay_id)
    .bind(&activity.id)
    .fetch_optional(db)
    .await?)
}

/// A relay's activity, newest first.
pub async fn list_activity_for_relay(
    db: impl PgExecutor<'_>,
    relay_id: &str,
) -> Result<Vec<Activity>> {
    Ok(sqlx::query_as::<_, Activity>(
        "SELECT * FROM activity WHERE relay_id = $1 ORDER BY created_at DESC, seq DESC",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}
