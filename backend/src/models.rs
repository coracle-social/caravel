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
    pub tenant_nwc_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewTenant {
    pub pubkey: String,
    pub status: String,
    pub tenant_nwc_url: String,
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
pub struct Invoice {
    pub id: String,
    pub tenant: String,
    pub amount: i64,
    pub status: String,
    pub created_at: String,
    pub invoice: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewInvoice {
    pub id: String,
    pub tenant: String,
    pub amount: i64,
    pub status: String,
    pub created_at: String,
    pub invoice: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct InvoiceItem {
    pub id: String,
    pub invoice: String,
    pub relay: String,
    pub amount: i64,
    pub period_start: String,
    pub period_end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewInvoiceItem {
    pub id: String,
    pub invoice: String,
    pub relay: String,
    pub amount: i64,
    pub period_start: String,
    pub period_end: String,
}
