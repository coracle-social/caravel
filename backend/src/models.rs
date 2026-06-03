use serde::{Deserialize, Serialize};
use sqlx::types::Json;

pub const RELAY_STATUS_ACTIVE: &str = "active";
pub const RELAY_STATUS_INACTIVE: &str = "inactive";
pub const RELAY_STATUS_DELINQUENT: &str = "delinquent";

/// Per-resource_type snapshot of a resource's state captured on each activity,
/// stored as JSON in `activity.snapshot`. Tagged on `resource_type` so the JSON
/// is self-describing and the variant matches the activity row's column. Add a
/// variant per resource type that needs state preserved on the activity log.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "resource_type", rename_all = "snake_case")]
pub enum Snapshot {
    Relay { plan: String, status: String },
}

impl Snapshot {
    pub fn resource_type(&self) -> &'static str {
        match self {
            Self::Relay { .. } => "relay",
        }
    }
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
    /// Last NWC auto-payment error, or `None` when the wallet last paid (or has
    /// never been tried). Surfaced in the UI to warn the user; it never blocks a
    /// retry — the next reconcile attempts payment again regardless. Also cleared
    /// when the tenant updates their NWC credentials.
    pub nwc_error: Option<String>,
    /// Last Stripe auto-payment error, with the same semantics as `nwc_error`.
    pub stripe_error: Option<String>,
    pub created_at: i64,
    pub billing_anchor: Option<i64>,
    pub stripe_customer_id: String,
    /// The tenant's saved Stripe payment method, or `None` if they have not set
    /// up a card yet. Set when the tenant adds a card via the Stripe portal.
    pub stripe_payment_method_id: Option<String>,
    /// `period_start` of the most recent period this tenant was renewed for, or
    /// `None` if never renewed. The per-period renewal idempotency marker.
    pub renewed_at: Option<i64>,
    /// When the tenant was churned because an invoice went unpaid past the grace
    /// period; its relays are delinquent while this is set. Cleared when billing
    /// is re-activated (the tenant has new billable activity), at which point the
    /// then-open invoices are voided rather than collected. `None` in good standing.
    pub churned_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Activity {
    pub id: String,
    pub tenant_pubkey: String,
    pub created_at: i64,
    pub activity_type: String,
    pub resource_type: String,
    pub resource_id: String,
    pub billed_at: Option<i64>,
    pub snapshot: Json<Snapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Relay {
    pub id: String,
    pub tenant_pubkey: String,
    pub subdomain: String,
    pub plan_id: String,
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
            tenant_pubkey: String::new(),
            subdomain: String::new(),
            plan_id: String::new(),
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

/// A tenant's bill for one period. Its lifecycle is recorded as timestamps
/// rather than a status column: open while both `paid_at` and `voided_at` are
/// null, paid once `paid_at` is set, and void once `voided_at` is set (e.g. a
/// balance forgiven when the tenant churns).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Invoice {
    pub id: String,
    pub tenant_pubkey: String,
    /// The total owed, fixed when the invoice is cut from its outstanding line
    /// items, so collection never has to re-sum them.
    pub amount: i64,
    pub period_start: i64,
    pub period_end: i64,
    pub created_at: i64,
    pub paid_at: Option<i64>,
    pub voided_at: Option<i64>,
    /// When the manual-payment reminder DM was sent for this invoice, or `None` if
    /// it hasn't been sent in order to avoid duplicate reminders for the same invoice.
    pub notified_at: Option<i64>,
    /// How the invoice was paid — `nwc`, `stripe`, or `oob` (out-of-band
    /// Lightning) — set when it is marked paid; `None` while open or void.
    pub method: Option<String>,
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
    pub plan_id: String,
    pub amount: i64,
    pub description: String,
    pub created_at: i64,
    /// Set when the item is forgiven — the tenant churned or reactivated — so it
    /// is never billed or carried into a later invoice; `None` while live. Applies
    /// whether or not the item has been claimed onto an invoice.
    pub voided_at: Option<i64>,
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

/// A hosted Stripe Checkout session opened to pay an invoice on-session (so a 3D
/// Secure challenge can be cleared), shaped like [`Bolt11`]: created pending and
/// stamped `settled_at` once paid. `id` is our uuid; `session_id` is the Stripe
/// Checkout Session (`cs_…`), used to reconcile and expire it; `url` is the
/// hosted page we redirect the tenant to.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Checkout {
    pub id: String,
    pub invoice_id: String,
    pub session_id: String,
    pub url: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub settled_at: Option<i64>,
}

/// A write-ahead record of an off-session card charge: inserted before the Stripe
/// call and stamped `settled_at` once the charge confirms and the invoice is paid.
/// `payment_method_id` is the method it charges, so a retry after a lost settle
/// re-confirms the same (idempotent) PaymentIntent rather than charging a second
/// one; `payment_intent_id` is the Stripe `pi_…` that settled it.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Intent {
    pub id: String,
    pub invoice_id: String,
    pub payment_method_id: String,
    pub payment_intent_id: Option<String>,
    pub created_at: i64,
    pub settled_at: Option<i64>,
}
