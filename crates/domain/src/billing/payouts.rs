//! Payouts: the Stripe Connect account a tenant's members pay into, less the
//! platform's fee.

use serde::{Deserialize, Serialize};

/// Whether a tenant can be paid, and what the platform keeps of each payment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PayoutStatus {
    pub account_id: Option<String>,
    pub ready: bool,
    pub fee_percent: f64,
}
