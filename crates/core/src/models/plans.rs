//! The plans relays are sold on.

use serde::{Deserialize, Serialize};

// Past charges are priced by their plan id, so ids are never reused or repriced.
pub const PLAN_FREE: &str = "free";

pub const PLAN_BASIC: &str = "basic";

pub const PLAN_GROWTH: &str = "growth";

/// One gigabyte, decimal — the unit storage is sold in.
pub const GB: i64 = 1_000_000_000;

/// Seconds in the participant-hour, the unit A/V call time is sold in.
pub const SECONDS_PER_HOUR: i64 = 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub name: String,
    pub amount: i64,
    pub members: Option<i64>,
    pub blossom: bool,
    pub livekit: bool,
    /// Blossom storage `amount` covers, in GB. A period bills the whole GB its
    /// peak usage stood above this, at `storage_overage` each.
    pub storage_gb: i64,
    /// Minor units per GB per period for storage past `storage_gb`.
    pub storage_overage: i64,
    /// A/V participant hours `amount` covers per period, counted per participant — four
    /// people on a half-hour call spend two. A period bills the whole hours it ran
    /// above this, at `av_overage` each.
    pub av_hours: i64,
    /// Minor units per A/V call hour past `av_hours`.
    pub av_overage: i64,
}

impl Plan {
    /// Whether relays on the plan are billed for it.
    pub fn is_paid(&self) -> bool {
        self.amount > 0
    }
}

/// The plan catalog. Paid tiers sell an allowance of storage and A/V participant
/// hours rather than a cap: usage past it bills per whole unit at the tier's rate,
/// priced so a relay that outgrows Basic reaches Growth's price around the point
/// Growth becomes the cheaper home for it.
pub fn list_plans() -> Vec<Plan> {
    vec![
        Plan {
            id: PLAN_FREE.to_string(),
            name: "Free".to_string(),
            amount: 0,
            members: Some(10),
            blossom: false,
            livekit: false,
            storage_gb: 0,
            storage_overage: 0,
            av_hours: 0,
            av_overage: 0,
        },
        Plan {
            id: PLAN_BASIC.to_string(),
            name: "Basic".to_string(),
            amount: 500,
            members: Some(100),
            blossom: true,
            livekit: true,
            storage_gb: 1,
            storage_overage: 200,
            av_hours: 30,
            av_overage: 10,
        },
        Plan {
            id: PLAN_GROWTH.to_string(),
            name: "Growth".to_string(),
            amount: 2500,
            members: None,
            blossom: true,
            livekit: true,
            storage_gb: 10,
            storage_overage: 75,
            av_hours: 300,
            av_overage: 4,
        },
    ]
}

pub fn get_plan(plan_id: &str) -> Option<Plan> {
    list_plans().into_iter().find(|plan| plan.id == plan_id)
}
