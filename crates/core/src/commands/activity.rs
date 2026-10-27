//! Writing a relay's activity log.

use sqlx::types::Json;
use sqlx::{PgConnection, PgExecutor};

use crate::commands;
use crate::error::Result;
use crate::models::activity::{Activity, ActivityType, Snapshot};
use crate::models::relays::{Relay, RelayStatus};

/// Record an activity on a relay, with a snapshot of its plan and `status`.
pub async fn insert_relay_activity(
    conn: &mut PgConnection,
    activity_type: ActivityType,
    relay: &Relay,
    status: RelayStatus,
    now: i64,
) -> Result<Activity> {
    let snapshot = Snapshot {
        plan_id: relay.plan_id.clone(),
        status,
    };
    Ok(sqlx::query_as::<_, Activity>(
        "INSERT INTO activity (id, tenant_pubkey, created_at, activity_type, relay_id, snapshot)
         SELECT $1, tenant_pubkey, $2, $3, id, $4 FROM relay WHERE id = $5
         RETURNING *",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(now)
    .bind(activity_type)
    .bind(Json(&snapshot))
    .bind(&relay.id)
    .fetch_one(conn)
    .await?)
}

/// Set a relay's status and record the activity that did it.
pub async fn record_status_change(
    conn: &mut PgConnection,
    relay: &Relay,
    status: RelayStatus,
    activity_type: ActivityType,
    now: i64,
) -> Result<Activity> {
    commands::relays::set_relay_status(&mut *conn, &relay.id, status).await?;
    insert_relay_activity(conn, activity_type, relay, status, now).await
}

/// Claim an activity as billed. Returns whether this call claimed it; `false`
/// means a concurrent pass already did, and its line item must not be repeated.
pub async fn mark_activity_billed(
    db: impl PgExecutor<'_>,
    activity_id: &str,
    now: i64,
) -> Result<bool> {
    let result =
        sqlx::query("UPDATE activity SET billed_at = $1 WHERE id = $2 AND billed_at IS NULL")
            .bind(now)
            .bind(activity_id)
            .execute(db)
            .await?;
    Ok(result.rows_affected() > 0)
}

/// Mark all of a tenant's unbilled activity billed except purges, whose credit
/// stays owed to the tenant.
pub async fn forgive_unbilled_activity_for_tenant(
    db: impl PgExecutor<'_>,
    tenant_pubkey: &str,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE activity SET billed_at = $1
         WHERE tenant_pubkey = $2 AND billed_at IS NULL AND activity_type <> 'purge_relay'",
    )
    .bind(now)
    .bind(tenant_pubkey)
    .execute(db)
    .await?;
    Ok(())
}
