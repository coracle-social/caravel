//! The relay activity log: one entry per change to a relay that billing prices,
//! each with a snapshot of the relay's plan and status as of that change.

use serde::{Deserialize, Serialize};
use sqlx::types::Json;

use crate::relays::RelayStatus;

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

/// A relay's plan and status as of an activity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub plan_id: String,
    pub status: RelayStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Activity {
    pub id: String,
    pub tenant_pubkey: String,
    pub created_at: i64,
    pub activity_type: ActivityType,
    pub relay_id: String,
    /// When billing reconciled the activity into the ledger, or `None` before.
    pub billed_at: Option<i64>,
    pub snapshot: Json<Snapshot>,
}
