use serde::{Deserialize, Serialize};

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
    pub sats: i64,
    pub members: Option<i64>,
    pub blossom: bool,
    pub livekit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tenant {
    pub pubkey: String,
    #[serde(skip_serializing)]
    pub nwc_url: String,
    pub created_at: i64,
    pub billing_anchor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Relay {
    pub id: String,
    pub tenant: String,
    pub schema: String,
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
    #[serde(skip_serializing)]
    pub synced: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Invoice {
    pub id: String,
    pub tenant: String,
    pub status: String,
    pub created_at: i64,
    pub attempted_at: i64,
    pub error: String,
    pub closed_at: i64,
    pub sent_at: i64,
    pub paid_at: i64,
    pub bolt11: String,
    pub period_start: i64,
    pub period_end: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct InvoiceItem {
    pub id: String,
    pub invoice: String,
    pub relay: String,
    pub sats: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InvoiceWithItems {
    #[serde(flatten)]
    pub invoice: Invoice,
    pub items: Vec<InvoiceItem>,
}
