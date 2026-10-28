//! Reading metered usage.

use sqlx::PgExecutor;

use crate::relays::RelayUsageTotals;
use caravel_util::error::Result;

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
