//! Tenants: the pubkeys that host relays on the platform and pay for them.

#[derive(Debug, Default, Clone, sqlx::FromRow)]
pub struct Tenant {
    pub pubkey: String,
    /// Where the robot's DMs send this tenant: their account page in whichever
    /// client they signed up through, supplied at creation. Used as-is for the
    /// churn DM, and with `?invoice=<id>` appended for a payment reminder.
    pub return_url: String,
    /// The tenant's NWC url, encrypted with the platform key, if they set one.
    pub nwc_url: Option<String>,
    /// Last NWC auto-payment error, or `None` when the wallet last paid or has
    /// never been tried. Shown as a warning; it never blocks a retry. Cleared when
    /// the tenant updates their NWC url.
    pub nwc_error: Option<String>,
    /// Last Stripe auto-payment error, with the same semantics as `nwc_error`.
    pub stripe_error: Option<String>,
    pub created_at: i64,
    /// When the tenant's first billable activity happened, which their monthly
    /// billing periods are counted from. `None` until then.
    pub billing_anchor: Option<i64>,
    pub stripe_customer_id: String,
    /// The saved Stripe payment method collection charges, or `None` before the
    /// tenant adds a card in the Stripe portal.
    pub stripe_payment_method_id: Option<String>,
    /// `period_start` of the latest period the tenant was renewed for: the
    /// per-period renewal idempotency marker.
    pub renewed_at: Option<i64>,
    /// `period_start` of the latest closed period whose metered overage was billed.
    /// Overage is billed once its period is over, so this trails `renewed_at`.
    pub overage_billed_at: Option<i64>,
    /// When the tenant was churned for an invoice unpaid past the grace period. Its
    /// relays are delinquent while this is set. Cleared when the tenant has new
    /// billable activity, which voids the then-open invoices.
    pub churned_at: Option<i64>,
    /// When an admin banned the tenant. A ban blocks relay creation, forgives
    /// outstanding charges and purges every relay; unbanning restores none of it.
    pub banned_at: Option<i64>,
    /// The Stripe Connect account the tenant's members pay into, once they start
    /// onboarding.
    pub stripe_account_id: Option<String>,
    /// Whether that account can take charges and receive payouts, as Stripe last
    /// said.
    pub stripe_account_ready: bool,
}

impl Tenant {
    /// The Connect account the tenant's members can pay into now, if any.
    pub fn payout_account(&self) -> Option<&str> {
        self.stripe_account_id
            .as_deref()
            .filter(|_| self.stripe_account_ready)
    }
}
