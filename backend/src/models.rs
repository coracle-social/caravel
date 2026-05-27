use serde::{Deserialize, Serialize};

pub const RELAY_STATUS_ACTIVE: &str = "active";
pub const RELAY_STATUS_INACTIVE: &str = "inactive";
pub const RELAY_STATUS_DELINQUENT: &str = "delinquent";

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Activity {
    pub id: String,
    pub tenant: String,
    pub created_at: i64,
    pub activity_type: String,
    pub resource_type: String,
    pub resource_id: String,
    pub billed_at: Option<i64>,
    /// The relay's plan at the time of a `create_relay`/`update_relay` activity;
    /// `None` for all other activity types.
    pub plan_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub name: String,
    pub amount: i64,
    pub members: Option<i64>,
    pub blossom: bool,
    pub livekit: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tenant {
    pub pubkey: String,
    pub nwc_url: String,
    pub nwc_error: Option<String>,
    pub created_at: i64,
    pub billing_anchor: Option<i64>,
    pub stripe_customer_id: String,
    /// `period_start` of the most recent period this tenant was renewed for, or
    /// `None` if never renewed. The per-period renewal idempotency marker.
    pub renewed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Relay {
    pub id: String,
    pub tenant: String,
    pub subdomain: String,
    pub plan: String,
    pub status: String,
    pub sync_error: String,
    pub info_name: String,
    pub info_icon: String,
    pub info_description: String,
    pub policy_public_join: i64,
    pub policy_strip_signatures: i64,
    pub groups_enabled: i64,
    pub management_enabled: i64,
    pub blossom_enabled: i64,
    pub livekit_enabled: i64,
    pub push_enabled: i64,
    pub synced: i64,
}

impl Default for Relay {
    fn default() -> Self {
        Self {
            id: String::new(),
            tenant: String::new(),
            subdomain: String::new(),
            plan: String::new(),
            status: RELAY_STATUS_ACTIVE.to_string(),
            sync_error: String::new(),
            info_name: String::new(),
            info_icon: String::new(),
            info_description: String::new(),
            policy_public_join: 0,
            policy_strip_signatures: 0,
            groups_enabled: 1,
            management_enabled: 1,
            blossom_enabled: 0,
            livekit_enabled: 0,
            push_enabled: 1,
            synced: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Invoice {
  pub id: String,
  pub tenant_pubkey: String,
  pub status: String,
  pub period_start: i64,
  pub period_end: i64,
  pub created_at: i64,
  pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct InvoiceItem {
  pub id: String,
  /// `None` while outstanding; set once the item is claimed onto an invoice.
  pub invoice_id: Option<String>,
  /// `None` for renewal items, which have no source activity.
  pub activity_id: Option<String>,
  pub tenant_pubkey: String,
  pub relay_id: String,
  pub plan: String,
  pub amount: i64,
  pub description: String,
  pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Bolt11 {
  pub id: String,
  pub invoice_id: String,
  pub lnbc: String,
  pub msats: i64,
  pub created_at: i64,
  pub expires_at: i64,
  pub settled_at: Option<i64>,
}

#[allow(dead_code)] // backs the `intent` table for the (not yet implemented) Stripe intent flow
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Intent {
  pub id: String,
  pub invoice_id: String,
  pub created_at: i64,
}
