//! The billing ledger: line items, the invoices that claim them, and the payment
//! attempts made against an invoice.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "text", rename_all = "lowercase")]
pub enum InvoiceMethod {
    Nwc,
    Stripe,
    /// A Lightning invoice paid out of band, such as by scanning its QR code.
    Oob,
    /// Comped: issued paid and never collected.
    Comp,
}

/// A tenant's bill for one period. It is open while `paid_at` and `voided_at` are
/// both null, paid once `paid_at` is set, and void once `voided_at` is.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Invoice {
    pub id: String,
    pub tenant_pubkey: String,
    /// The total owed, fixed when the invoice claims its line items.
    pub amount: i64,
    pub period_start: i64,
    pub period_end: i64,
    pub created_at: i64,
    pub paid_at: Option<i64>,
    pub voided_at: Option<i64>,
    /// When the manual-payment reminder DM was last sent for this invoice.
    pub notified_at: Option<i64>,
    /// How the invoice was paid, set when it is marked paid.
    pub method: Option<InvoiceMethod>,
}

impl Invoice {
    pub fn is_open(&self) -> bool {
        self.paid_at.is_none() && self.voided_at.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct InvoiceItem {
    pub id: String,
    /// `None` while outstanding; set once an invoice claims the item.
    pub invoice_id: Option<String>,
    /// The activity the item prices, or `None` for renewals and overage.
    pub activity_id: Option<String>,
    pub tenant_pubkey: String,
    pub relay_id: String,
    pub plan_id: String,
    pub amount: i64,
    pub description: String,
    pub created_at: i64,
    /// When the item was forgiven, because the tenant churned or came back, so it
    /// is never billed or carried into a later invoice.
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

/// A hosted Stripe Checkout session opened to pay an invoice on-session, so the
/// tenant can clear a 3D Secure challenge. `session_id` is Stripe's `cs_…` id and
/// `url` the hosted page.
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

/// A write-ahead record of an off-session card charge on `payment_method_id`,
/// inserted before the Stripe call and dropped once Stripe refuses it. The charge
/// is idempotent on `id`, so a retry after a lost settle or an unanswered call
/// replays the same PaymentIntent; `payment_intent_id` is the Stripe `pi_…` that
/// settled it.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Intent {
    pub id: String,
    pub invoice_id: String,
    pub payment_method_id: String,
    pub payment_intent_id: Option<String>,
    pub created_at: i64,
    pub settled_at: Option<i64>,
}
