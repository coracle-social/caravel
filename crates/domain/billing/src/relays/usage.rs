//! Metered usage: hourly readings of each paid relay's storage and A/V call time,
//! rolled up per billing period for overage.

/// One reading of a relay's metered usage. `storage_bytes` is a level, so a
/// period bills its peak; `participant_seconds` is a lifetime clock of time spent
/// in calls, so a period bills how far it advanced, which stays right across
/// readings that were missed.
#[derive(Debug, Clone, sqlx::FromRow)]
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
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RelayUsageTotals {
    pub relay_id: String,
    pub peak_storage_bytes: i64,
    pub participant_seconds: i64,
}
