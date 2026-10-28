//! Routes for tenants managing their own account, relays and invoices. An admin
//! may call each of them on any tenant's behalf.

pub mod account;
pub mod invoices;
pub mod payouts;
pub mod relays;

pub use account::*;
pub use invoices::*;
pub use payouts::*;
pub use relays::*;

use caravel_billing::tenants::Tenant;
use serde::Serialize;

/// A tenant as the API shows it, which reveals whether an NWC url is set but not
/// the url.
#[derive(Serialize)]
pub struct TenantView {
    pub pubkey: String,
    pub return_url: String,
    pub nwc_is_set: bool,
    pub nwc_error: Option<String>,
    pub stripe_error: Option<String>,
    pub created_at: i64,
    pub billing_anchor: Option<i64>,
    pub stripe_customer_id: String,
    pub stripe_payment_method_id: Option<String>,
    pub churned_at: Option<i64>,
    pub banned_at: Option<i64>,
}

impl From<Tenant> for TenantView {
    fn from(tenant: Tenant) -> Self {
        Self {
            nwc_is_set: tenant.nwc_url.is_some(),
            pubkey: tenant.pubkey,
            return_url: tenant.return_url,
            nwc_error: tenant.nwc_error,
            stripe_error: tenant.stripe_error,
            created_at: tenant.created_at,
            billing_anchor: tenant.billing_anchor,
            stripe_customer_id: tenant.stripe_customer_id,
            stripe_payment_method_id: tenant.stripe_payment_method_id,
            churned_at: tenant.churned_at,
            banned_at: tenant.banned_at,
        }
    }
}
