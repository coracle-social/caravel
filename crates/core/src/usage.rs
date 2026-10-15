//! Metered usage: hourly readings of each paid relay's storage and A/V call time,
//! rolled up per billing period for overage.

use serde::{Deserialize, Serialize};
use sqlx::PgExecutor;

use crate::Core;
use crate::error::Result;
use crate::plans;
use crate::relays::{self, RelayStatus};

/// One reading of a relay's metered usage. `storage_bytes` is a level, so a
/// period bills its peak; `participant_seconds` is a lifetime clock of time spent
/// in calls, so a period bills how far it advanced, which stays right across
/// readings that were missed.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct UsageSample {
    pub id: String,
    pub relay_id: String,
    pub tenant_pubkey: String,
    pub storage_bytes: i64,
    pub participant_seconds: i64,
    pub created_at: i64,
}

/// One relay's usage across a closed billing period: storage at its peak, and the
/// call time its clock advanced.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RelayUsageTotals {
    pub relay_id: String,
    pub peak_storage_bytes: i64,
    pub participant_seconds: i64,
}

pub async fn insert_usage_sample(db: impl PgExecutor<'_>, sample: &UsageSample) -> Result<()> {
    sqlx::query(
        "INSERT INTO usage_sample (id, relay_id, tenant_pubkey, storage_bytes, participant_seconds, created_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&sample.id)
    .bind(&sample.relay_id)
    .bind(&sample.tenant_pubkey)
    .bind(sample.storage_bytes)
    .bind(sample.participant_seconds)
    .bind(sample.created_at)
    .execute(db)
    .await?;
    Ok(())
}

/// Every metered relay's usage across `[start, end)` for a tenant. Call time is
/// measured from the relay's last sample before the period, so calls between the
/// boundary and the period's first sample aren't lost, falling back to the
/// period's own first reading. Clamped at zero so a clock that restarted reads as
/// no usage rather than a credit.
pub async fn get_usage_totals_for_tenant(
    db: impl PgExecutor<'_>,
    tenant_pubkey: &str,
    start: i64,
    end: i64,
) -> Result<Vec<RelayUsageTotals>> {
    Ok(sqlx::query_as::<_, RelayUsageTotals>(
        "SELECT
           s.relay_id,
           MAX(s.storage_bytes) AS peak_storage_bytes,
           GREATEST(0, MAX(s.participant_seconds) - COALESCE(
             (SELECT p.participant_seconds FROM usage_sample p
               WHERE p.relay_id = s.relay_id AND p.created_at < $2
               ORDER BY p.created_at DESC LIMIT 1),
             MIN(s.participant_seconds)
           )) AS participant_seconds
         FROM usage_sample s
         WHERE s.tenant_pubkey = $1 AND s.created_at >= $2 AND s.created_at < $3
         GROUP BY s.relay_id",
    )
    .bind(tenant_pubkey)
    .bind(start)
    .bind(end)
    .fetch_all(db)
    .await?)
}

/// Take a usage reading of every active relay on a paid plan with a metered
/// feature on. A relay that can't be read is skipped, leaving a gap its period's
/// rollup tolerates.
pub async fn sample_usage(core: &Core) -> Result<()> {
    let now = core.clock.now();

    for relay in relays::list_relays(core.db.pool()).await? {
        if relay.is_purged() || relay.status != RelayStatus::Active {
            continue;
        }
        if !relay.blossom_enabled && !relay.livekit_enabled {
            continue;
        }
        if plans::get_plan(&relay.plan_id).is_none_or(|plan| plan.amount <= 0) {
            continue;
        }

        let usage = match core.zooid.get_usage(&relay.id).await {
            Ok(usage) => usage,
            Err(error) => {
                tracing::warn!(relay = %relay.id, error = %error, "failed to sample relay usage");
                continue;
            }
        };

        let sample = UsageSample {
            id: uuid::Uuid::new_v4().to_string(),
            relay_id: relay.id.clone(),
            tenant_pubkey: relay.tenant_pubkey.clone(),
            storage_bytes: usage.storage_bytes,
            participant_seconds: usage.participant_seconds,
            created_at: now,
        };

        if let Err(error) = insert_usage_sample(core.db.pool(), &sample).await {
            tracing::error!(relay = %relay.id, error = %error, "failed to record usage sample");
        }
    }

    Ok(())
}
