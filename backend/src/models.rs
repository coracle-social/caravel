use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    pub policy: Option<serde_json::Value>,
    pub groups: Option<serde_json::Value>,
    pub management: Option<serde_json::Value>,
    pub blossom: Option<serde_json::Value>,
    pub livekit: Option<serde_json::Value>,
    pub push: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tenant {
    pub pubkey: String,
    pub status: String,
    pub nwc_url: String,
    pub created_at: i64,
    pub billing_anchor_at: i64,
    pub stripe_customer_id: String,
    pub stripe_subscription_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relay {
    pub id: String,
    pub tenant: String,
    pub name: String,
    pub subdomain: String,
    pub icon: String,
    pub description: String,
    pub plan: String,
    pub status: String,
    pub config: Option<RelayConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Plan {
    pub id: String,
    pub sats_per_month: i64,
}

/// Append-only record of relay lifecycle transitions (provisioned, suspended,
/// unsuspended, deactivated). Used as the source of truth for usage metering.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RelayLifecycleEvent {
    pub id: String,
    pub relay: String,
    pub tenant: String,
    /// One of: "provisioned", "suspended", "unsuspended", "deactivated"
    pub event_type: String,
    /// Plan active on the relay at the time of the event
    pub plan: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Invoice {
    pub id: String,
    pub tenant: String,
    pub amount: i64,
    /// One of: "pending", "past_due", "paid", "void"
    pub status: String,
    pub created_at: i64,
    /// bolt11 invoice string (may be refreshed in-app when expired)
    pub bolt11: String,
    pub period_start: i64,
    pub period_end: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct InvoiceItem {
    pub id: String,
    pub invoice: String,
    pub relay: String,
    pub amount: i64,
    pub period_start: i64,
    pub period_end: i64,
}

/// Canonical history of payment attempts. `invoices.status` is a synchronous
/// projection updated in the same transaction as each new attempt row.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct InvoiceAttempt {
    pub id: String,
    pub invoice: String,
    /// Groups all method attempts within a single collection run
    pub run_id: String,
    /// One of: "nwc", "stripe", "lightning", "nip17_dm"
    pub method: String,
    /// One of: "success", "failed", "sent" (for DM)
    pub outcome: String,
    pub error: String,
    pub created_at: i64,
}
