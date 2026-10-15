//! The relay activity log: one row per change to a relay that billing prices,
//! each with a snapshot of the relay's plan and status as of that change.

use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::{PgConnection, PgExecutor};

use crate::error::Result;
use crate::relays::{self, Relay, RelayStatus};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum ActivityType {
    CreateRelay,
    UpdateRelay,
    ActivateRelay,
    DeactivateRelay,
    PurgeRelay,
    MarkRelayDelinquent,
    UnmarkRelayDelinquent,
}

/// A resource's state as of an activity, tagged with its resource type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "resource_type", rename_all = "snake_case")]
pub enum Snapshot {
    Relay { plan: String, status: RelayStatus },
}

impl Snapshot {
    pub fn resource_type(&self) -> &'static str {
        match self {
            Self::Relay { .. } => "relay",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Activity {
    pub id: String,
    pub tenant_pubkey: String,
    pub created_at: i64,
    pub activity_type: ActivityType,
    pub resource_type: String,
    pub resource_id: String,
    /// When billing reconciled the activity into the ledger, or `None` before.
    pub billed_at: Option<i64>,
    pub snapshot: Json<Snapshot>,
}

/// Record an activity on a relay, with a snapshot of its plan and status.
pub async fn insert_relay_activity(
    conn: &mut PgConnection,
    activity_type: ActivityType,
    relay_id: &str,
    snapshot: Snapshot,
    now: i64,
) -> Result<Activity> {
    Ok(sqlx::query_as::<_, Activity>(
        "INSERT INTO activity (id, tenant_pubkey, created_at, activity_type, resource_type, resource_id, snapshot)
         SELECT $1, tenant_pubkey, $2, $3, $4, id, $5 FROM relay WHERE id = $6
         RETURNING *",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(now)
    .bind(activity_type)
    .bind(snapshot.resource_type())
    .bind(Json(&snapshot))
    .bind(relay_id)
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
    relays::set_relay_status(&mut *conn, &relay.id, status).await?;
    let snapshot = Snapshot::Relay {
        plan: relay.plan_id.clone(),
        status,
    };
    insert_relay_activity(conn, activity_type, &relay.id, snapshot, now).await
}

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
         WHERE resource_id = $1 AND resource_type = 'relay' AND created_at < $2
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
         WHERE resource_id = $1 AND resource_type = 'relay'
           AND (created_at, seq) < (SELECT created_at, seq FROM activity WHERE id = $2)
         ORDER BY created_at DESC, seq DESC
         LIMIT 1",
    )
    .bind(&activity.resource_id)
    .bind(&activity.id)
    .fetch_optional(db)
    .await?)
}

/// A resource's activity, newest first.
pub async fn list_activity_for_resource(
    db: impl PgExecutor<'_>,
    resource_id: &str,
) -> Result<Vec<Activity>> {
    Ok(sqlx::query_as::<_, Activity>(
        "SELECT * FROM activity WHERE resource_id = $1 ORDER BY created_at DESC, seq DESC",
    )
    .bind(resource_id)
    .fetch_all(db)
    .await?)
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
