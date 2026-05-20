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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub name: String,
    pub amount: i64,
    pub members: Option<i64>,
    pub blossom: bool,
    pub livekit: bool,
    pub stripe_price_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tenant {
    pub pubkey: String,
    pub nwc_url: String,
    pub nwc_error: Option<String>,
    pub created_at: i64,
    pub stripe_customer_id: String,
    pub stripe_subscription_id: Option<String>,
    pub past_due_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Relay {
    pub id: String,
    pub tenant: String,
    pub schema: String,
    pub subdomain: String,
    pub plan: String,
    pub stripe_subscription_item_id: Option<String>,
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
            schema: String::new(),
            subdomain: String::new(),
            plan: String::new(),
            stripe_subscription_item_id: None,
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
