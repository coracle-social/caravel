use anyhow::{Result, anyhow};
use std::time::Duration;

use crate::bitcoin;
use crate::command;
use crate::env;
use crate::models::{
    Activity, Bolt11, Checkout, Invoice, InvoiceItem, RELAY_STATUS_ACTIVE, Snapshot, Tenant,
};
use crate::query;
use crate::robot::Robot;
use crate::stripe::Stripe;
use crate::wallet::Wallet;

const POLL_INTERVAL: Duration = Duration::from_secs(60 * 60);
const GRACE_PERIOD_SECS: i64 = 7 * 24 * 60 * 60;
/// Hold the manual-payment DM until an open invoice is at least this old. A freshly
/// issued invoice is surfaced to the tenant in-app first (e.g. right after they
/// create a relay), so we don't also nag by DM on the first dunning cycles.
const FRESH_INVOICE_DM_GRACE_SECS: i64 = 24 * 60 * 60;
const MANUAL_PAYMENT_DM_INTERVAL_SECS: i64 = 12 * 24 * 60 * 60;
const MANUAL_PAYMENT_DM: &str = "Payment is due for your relay subscription. Open the link below to review the invoice and pay by Lightning or card:";
const CHURN_DM: &str = "Your relay subscription is past due, so your relays have been paused. You can restore service any time by adding a payment method or paying an invoice from your dashboard:";

/// Owns subscription billing: it reconciles tenant activity into invoice items,
/// renews subscriptions each period, and collects payment (Lightning, then a
/// card on file, then a manual DM link).
#[derive(Clone)]
pub struct Billing {
    stripe: Stripe,
    wallet: Wallet,
    robot: Robot,
}

impl Billing {
    pub fn new(robot: Robot) -> Self {
        Self {
            stripe: Stripe::new(),
            wallet: Wallet::from_url(&env::get().robot_wallet).expect("invalid ROBOT_WALLET"),
            robot,
        }
    }

    // --- lifecycle methods ---

    pub async fn start(self) {
        let mut interval = tokio::time::interval(POLL_INTERVAL);

        loop {
            interval.tick().await;

            if let Err(error) = self.reconcile_subscriptions().await {
                tracing::error!(error = %error, "billing poll failed");
            }
        }
    }

    async fn reconcile_subscriptions(&self) -> Result<()> {
        let tenants = query::list_tenants().await?;

        tracing::info!(
            tenant_count = tenants.len(),
            "reconciling all subscriptions"
        );

        for tenant in tenants {
            if let Err(error) = self.reconcile_subscription(&tenant, true).await {
                tracing::error!(
                    tenant = %tenant.pubkey,
                    error = ?error,
                    "failed to reconcile subscription"
                );
            }
        }

        Ok(())
    }

    // --- Reconciliation of activity/renewals ---

    /// Reconciles a tenant's billing: re-activates them if a churned tenant has
    /// re-engaged, folds billable activity into line items (setting the billing
    /// anchor based on the first one), renews the current period if due, and claims
    /// outstanding items onto an invoice.
    pub async fn reconcile_subscription(
        &self,
        tenant: &Tenant,
        attempt_payment: bool,
    ) -> Result<()> {
        let mut tenant = tenant.clone();

        let activities = query::list_billable_activity(&tenant.pubkey).await?;

        // A churned tenant with fresh billable activity is using the service
        // again: re-activate billing (and restore their relays) before billing it.
        if tenant.churned_at.is_some() && !activities.is_empty() {
            let relays = query::list_relays_for_tenant(&tenant.pubkey).await?;
            command::reactivate_tenant(&tenant.pubkey, &relays).await?;
            tenant.churned_at = None;
        }

        // Reconcile all activity, setting the tenant's billing anchor on the first
        // activity if not already set.
        for activity in activities {
            if tenant.billing_anchor.is_none() {
                tenant.billing_anchor = Some(activity.created_at);
                command::set_tenant_billing_anchor(&tenant).await?;
            }

            self.reconcile_activity(&tenant, &activity).await?;
        }

        // If the tenant has a billing anchor, renew the current period if due and
        // claim any outstanding items onto an invoice.
        if let Some(period) = BillingPeriod::current(&tenant) {
            if tenant.renewed_at.is_none_or(|at| at < period.start) {
                self.reconcile_renewal(&tenant, &period).await?;
            }

            command::create_invoice(&tenant, &period).await?;
        }

        // Fetch the tenant's open invoices once
        let invoices = query::list_open_invoices(&tenant.pubkey).await?;

        // If the tenant is past due, churn them
        if self.maybe_churn_tenant(&tenant, &invoices).await? {
            return Ok(());
        }

        // If we're going to try to collect, make sure we have an updated payment method
        if attempt_payment {
            tenant.stripe_payment_method_id = self.sync_stripe_customer(&tenant).await?;
        }

        // Reconcile out-of-band payments (and, when collecting, charge) on every
        // open invoice. The out-of-band checks run even when attempt_payment is
        // false, so a checkout or bolt11 paid out of band settles on any reconcile.
        for invoice in &invoices {
            self.reconcile_payments(&tenant, invoice, attempt_payment, true)
                .await?;
        }

        Ok(())
    }

    /// Reconcile one activity into the ledger: build its line item (if any) and
    /// persist it with the activity's billed marker. Activities that produce no
    /// item (e.g. free-plan changes) are still marked billed so they aren't
    /// re-scanned.
    async fn reconcile_activity(&self, tenant: &Tenant, activity: &Activity) -> Result<()> {
        let invoice_item = match activity.activity_type.as_str() {
            "create_relay" => {
                self.make_prorated_item(tenant, activity, 1, "New relay created")
                    .await?
            }
            "activate_relay" => {
                self.make_prorated_item(tenant, activity, 1, "Relay reactivated")
                    .await?
            }
            "deactivate_relay" => {
                self.make_prorated_item(tenant, activity, -1, "Relay deactivated (prorated credit)")
                    .await?
            }
            "update_relay" => self.make_plan_change_item(tenant, activity).await?,
            _ => None,
        };

        match invoice_item {
            Some(ref item) => command::insert_invoice_item_for_activity(item, &activity.id).await,
            None => command::mark_activity_billed(&activity.id).await,
        }
    }

    /// A prorated charge (or credit, with `sign` = -1) for the plan recorded on
    /// the activity's snapshot — the plan as of the activity, not the relay's
    /// current plan — covering the fraction of the period remaining at the
    /// activity. `None` for a free plan.
    async fn make_prorated_item(
        &self,
        tenant: &Tenant,
        activity: &Activity,
        sign: i64,
        description: &str,
    ) -> Result<Option<InvoiceItem>> {
        let Snapshot::Relay { plan: plan_id, .. } = &*activity.snapshot;
        let plan = query::get_plan(plan_id)?;
        if plan.amount <= 0 {
            return Ok(None);
        }

        let period = BillingPeriod::at(tenant, activity.created_at)
            .ok_or_else(|| anyhow!("billing anchor must be set before building prorated items"))?;
        let amount = sign * period.prorate(plan.amount, activity.created_at);

        Ok(Some(InvoiceItem {
            id: uuid::Uuid::new_v4().to_string(),
            invoice_id: None,
            activity_id: Some(activity.id.clone()),
            tenant_pubkey: activity.tenant_pubkey.clone(),
            relay_id: activity.resource_id.clone(),
            plan_id: plan.id,
            amount,
            description: description.to_string(),
            created_at: activity.created_at,
            voided_at: None,
        }))
    }

    /// The prorated delta for a plan change, read straight from the activity log:
    /// `new` is this `update_relay` activity's recorded plan, `old` is the relay's
    /// plan immediately before it. Because the renewal charges the relay's plan as
    /// of the period boundary, this delta composes to the correct total regardless
    /// of ordering and needs no coverage gate. `None` when nothing changed.
    async fn make_plan_change_item(
        &self,
        tenant: &Tenant,
        activity: &Activity,
    ) -> Result<Option<InvoiceItem>> {
        let new_plan_id = match &*activity.snapshot {
            Snapshot::Relay { plan, .. } => plan,
        };
        let Some(old_plan_id) =
            query::get_relay_plan_before(&activity.resource_id, activity.created_at).await?
        else {
            return Err(anyhow!("no previous plan found for relay update activity"));
        };
        if &old_plan_id == new_plan_id {
            return Ok(None);
        }
        let new_plan = query::get_plan(new_plan_id)?;
        let old_plan = query::get_plan(&old_plan_id)?;

        let period = BillingPeriod::at(tenant, activity.created_at)
            .ok_or_else(|| anyhow!("billing anchor must be set before building prorated items"))?;
        let amount = period.prorate(new_plan.amount, activity.created_at)
            - period.prorate(old_plan.amount, activity.created_at);
        if amount == 0 {
            return Ok(None);
        }

        let description = format!("Plan changed from {} to {}", old_plan.name, new_plan.name);
        Ok(Some(InvoiceItem {
            id: uuid::Uuid::new_v4().to_string(),
            invoice_id: None,
            activity_id: Some(activity.id.clone()),
            tenant_pubkey: activity.tenant_pubkey.clone(),
            relay_id: activity.resource_id.clone(),
            plan_id: new_plan.id,
            amount,
            description,
            created_at: activity.created_at,
            voided_at: None,
        }))
    }

    /// Charge a full-period renewal for every relay that was active on a paid plan
    /// as of `period.start`, reading that state from each relay's most recent
    /// activity snapshot before the boundary (relays with no prior activity didn't
    /// exist yet and are skipped). Idempotent per period via the tenant's
    /// `renewed_at` marker, so calling it on every generation can't renew twice;
    /// a relay created/activated *within* the period isn't active before the
    /// boundary, so it's covered by its own prorated charge instead.
    async fn reconcile_renewal(&self, tenant: &Tenant, period: &BillingPeriod) -> Result<()> {
        let relays = query::list_relays_for_tenant(&tenant.pubkey).await?;

        let mut line_items = Vec::new();
        for relay in relays {
            let Some(activity) =
                query::get_latest_relay_activity_before(&relay.id, period.start).await?
            else {
                continue;
            };
            let Snapshot::Relay {
                plan: plan_id,
                status,
                ..
            } = &*activity.snapshot;
            if status != RELAY_STATUS_ACTIVE {
                continue;
            }
            let plan = query::get_plan(plan_id)?;
            if plan.amount <= 0 {
                continue;
            }
            line_items.push(InvoiceItem {
                id: uuid::Uuid::new_v4().to_string(),
                invoice_id: None,
                activity_id: None,
                tenant_pubkey: tenant.pubkey.clone(),
                relay_id: relay.id,
                plan_id: plan.id,
                amount: plan.amount,
                description: "Subscription renewal".to_string(),
                created_at: period.start,
                voided_at: None,
            });
        }

        // Inserts the items and advances `renewed_at` to `period.start` in one
        // transaction (idempotent via an in-tx guard), so a re-tick is a no-op.
        command::insert_invoice_items_for_renewal(&line_items, period).await
    }

    // --- Auto-churn ---

    /// Churn a tenant whose oldest open invoice has blown past the grace period:
    /// pause their relays and DM them once, on the transition into churn. Returns
    /// whether the tenant is past due, so the caller can skip collecting this pass.
    async fn maybe_churn_tenant(&self, tenant: &Tenant, invoices: &[Invoice]) -> Result<bool> {
        let Some(oldest) = invoices.first() else {
            return Ok(false);
        };

        let now = chrono::Utc::now().timestamp();
        if now - oldest.created_at < GRACE_PERIOD_SECS {
            return Ok(false);
        }

        // Past due. Churn once (the guard fires a single time on the transition)
        // and notify the tenant, logging and continuing on DM failure.
        if tenant.churned_at.is_none() {
            let relays = query::list_relays_for_tenant(&tenant.pubkey).await?;
            command::churn_tenant(&tenant.pubkey, now, &relays).await?;

            let message = format!("{CHURN_DM}\n\n{}/account", env::get().app_url);
            if let Err(e) = self.robot.send_dm(&tenant.pubkey, &message).await {
                tracing::error!(tenant = %tenant.pubkey, error = %e, "failed to send churn DM");
            }
        }

        Ok(true)
    }

    // --- Payments ---

    /// Collect an invoice. We check the out-of-band rails first — a Lightning
    /// invoice or Checkout session the tenant may have already paid — and only
    /// then initiate a fresh charge (NWC, then a saved card), so a payment that's
    /// already in flight is never duplicated. Falling all the way through sends a
    /// manual-payment DM (when `notify`). A failing charge's error is stored on
    /// the tenant (to warn them in the UI) but never aborts the cascade or future
    /// retries; it's cleared when a method next succeeds. Caller-initiated
    /// payments pass `notify = false` to skip the dunning DM, since the failure
    /// is already surfaced on screen.
    pub async fn reconcile_payments(
        &self,
        tenant: &Tenant,
        invoice: &Invoice,
        autopay: bool,
        notify: bool,
    ) -> Result<()> {
        let mut error_message: Option<String> = None;

        // 1. Out-of-band lightning: settle a bolt11 paid out of band (e.g. a
        // manual QR scan, or an NWC pay that completed but failed to record).
        // Checked before any charge so we never bill on top of it.
        if let Some(bolt11) = query::get_bolt11_for_invoice(&invoice.id).await?
            && bolt11.settled_at.is_none()
            && self.wallet.is_settled(&bolt11.lnbc).await.unwrap_or(false)
        {
            command::settle_invoice_out_of_band(&bolt11.id, &invoice.id).await?;
            return self.cleanup_pending_payments(invoice).await;
        }

        // 2. Hosted Checkout: settle an invoice the tenant paid (and
        // authenticated) on a Stripe Checkout session that has since completed.
        // Also checked before any charge so we never bill on top of it.
        if let Some(checkout) = query::get_checkout_for_invoice(&invoice.id).await?
            && checkout.settled_at.is_none()
            && self
                .stripe
                .is_checkout_paid(&checkout.id)
                .await
                .unwrap_or(false)
        {
            command::settle_invoice_via_checkout(&tenant.pubkey, &checkout.id, &invoice.id).await?;
            return self.cleanup_pending_payments(invoice).await;
        }

        if !autopay {
            return Ok(());
        }

        // 3. NWC auto-pay: if the tenant has configured an nwc_url, charge it.
        if !tenant.nwc_url.is_empty() {
            match self.attempt_payment_using_nwc(tenant, invoice).await {
                Ok(()) => return self.cleanup_pending_payments(invoice).await,
                Err(e) => error_message = Some(format!("{e}")),
            }
        }

        // 4. Payment method on file: charge the tenant's cached Stripe payment
        // method, kept fresh by sync_stripe_payment_method before collection.
        if let Some(payment_method) = &tenant.stripe_payment_method_id {
            match self
                .attempt_payment_using_stripe(tenant, invoice, payment_method)
                .await
            {
                Ok(()) => return self.cleanup_pending_payments(invoice).await,
                Err(e) => error_message = error_message.or_else(|| Some(format!("{e}"))),
            }
        }

        if !notify {
            return Ok(());
        }

        // 5. Manual payment: DM a link to the in-app payment page for this invoice.
        if let Err(e) = self
            .attempt_payment_using_dm(tenant, invoice, error_message)
            .await
        {
            tracing::error!(
                tenant = %tenant.pubkey,
                error = %e,
                "failed to send manual payment DM"
            );
        }

        Ok(())
    }

    async fn attempt_payment_using_nwc(&self, tenant: &Tenant, invoice: &Invoice) -> Result<()> {
        let result: Result<()> = async {
            let nwc_url = env::get().decrypt(&tenant.nwc_url)?;
            let tenant_wallet = Wallet::from_url(&nwc_url)?;
            let bolt11 = self.ensure_bolt11_for_invoice(invoice).await?;

            tenant_wallet.pay_invoice(bolt11.lnbc.clone()).await?;

            command::settle_invoice_via_nwc(&tenant.pubkey, &bolt11.id, &invoice.id).await
        }
        .await;

        // Record the failure on the tenant (to warn them in the UI) but still
        // surface it, so the cascade can fall through and summarize it in the DM.
        if let Err(error) = &result {
            command::set_tenant_nwc_error(&tenant.pubkey, &format!("{error}")).await?;
        }

        result
    }

    async fn attempt_payment_using_stripe(
        &self,
        tenant: &Tenant,
        invoice: &Invoice,
        payment_method_id: &str,
    ) -> Result<()> {
        let result: Result<()> = async {
            let intent_id = self
                .stripe
                .create_payment_intent(
                    &tenant.stripe_customer_id,
                    payment_method_id,
                    &invoice.id,
                    invoice.amount,
                    "usd",
                )
                .await?;

            command::settle_invoice_via_intent(&tenant.pubkey, &intent_id, &invoice.id).await
        }
        .await;

        // Record the failure on the tenant (to warn them in the UI) but still
        // surface it, so the cascade can fall through and summarize it in the DM.
        if let Err(error) = &result {
            command::set_tenant_stripe_error(&tenant.pubkey, &format!("{error}")).await?;
        }
        result
    }

    async fn attempt_payment_using_dm(
        &self,
        tenant: &Tenant,
        invoice: &Invoice,
        error: Option<String>,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();

        // If the invoice was just generated, give the user a chance to check the dashboard before nagging them.
        if now - invoice.created_at < FRESH_INVOICE_DM_GRACE_SECS {
            return Ok(());
        }

        // The dunning poll runs hourly; avoid excessive reminder DMs.
        if invoice
            .notified_at
            .is_some_and(|t| now - t < MANUAL_PAYMENT_DM_INTERVAL_SECS)
        {
            return Ok(());
        }

        // Build the message
        let invoice_id = &invoice.id;
        let url_base = &env::get().app_url;
        let payment_url = format!("{url_base}/account?invoice={invoice_id}");
        let base = format!("{MANUAL_PAYMENT_DM}\n\n{payment_url}");
        let dm_message = match error {
            Some(error) if !error.is_empty() => {
                let limit: usize = 240;
                let summary = error
                    .chars()
                    .take(limit.saturating_sub(3))
                    .collect::<String>();
                format!("{base}\n\nAuto-payment failed: {summary}")
            }
            _ => base,
        };

        // Send via NIP 17
        self.robot.send_dm(&tenant.pubkey, &dm_message).await?;

        // Record the send to avoid spammy notifications.
        command::mark_invoice_notified(invoice_id).await
    }

    /// Run after an invoice is settled to invalidate out-of-band payment methods
    /// so the tenant can't pay twice. Only Stripe Checkout sessions can actually
    /// be invalidated (by expiring them); Lightning/NWC has no such mechanism.
    /// The session that just paid (if any) was marked settled in its settle
    /// transaction, so it's excluded here and not needlessly expired.
    async fn cleanup_pending_payments(&self, invoice: &Invoice) -> Result<()> {
        for checkout in query::list_pending_checkouts_for_invoice(&invoice.id).await? {
            if let Err(error) = self.stripe.expire_checkout_session(&checkout.id).await {
                tracing::debug!(
                    invoice = %invoice.id,
                    checkout = %checkout.id,
                    error = %error,
                    "could not expire checkout session"
                );
            }
        }

        Ok(())
    }

    // --- Bolt11 utils ---

    pub async fn ensure_bolt11_for_invoice(&self, invoice: &Invoice) -> Result<Bolt11> {
        let now = chrono::Utc::now().timestamp();

        // A resolved (paid or voided) invoice must never mint a new payable
        // bolt11; return the existing one if present, otherwise refuse.
        if invoice.paid_at.is_some() || invoice.voided_at.is_some() {
            return query::get_bolt11_for_invoice(&invoice.id)
                .await?
                .ok_or_else(|| anyhow!("invoice is not open"));
        }

        if let Some(existing) = query::get_bolt11_for_invoice(&invoice.id).await?
            && (existing.settled_at.is_none() || now < existing.expires_at)
        {
            return Ok(existing);
        }

        let expiry: i64 = 3600;
        let info = "Relay subscription payment";
        let msats = bitcoin::fiat_to_msats(invoice.amount, "usd").await?;
        let lnbc = self.wallet.make_invoice(msats, info, expiry as u64).await?;

        command::insert_bolt11(&invoice.id, &lnbc, msats as i64, now + expiry)
            .await?
            .ok_or_else(|| anyhow!("failed to insert bolt11"))
    }

    // --- Checkout utils ---

    /// Idempotently produce a hosted Stripe Checkout session for an open invoice,
    /// reusing an unsettled, unexpired one if present — the on-session card
    /// counterpart to [`Self::ensure_bolt11_for_invoice`]. Checkout lets the
    /// tenant clear a 3D Secure challenge the off-session card charge can't. On
    /// success Stripe returns the tenant to their account page, where collection
    /// is reconciled (here and on the dunning poll) once the session reads paid.
    pub async fn ensure_checkout_for_invoice(
        &self,
        tenant: &Tenant,
        invoice: &Invoice,
    ) -> Result<Checkout> {
        let now = chrono::Utc::now().timestamp();

        // Never open a Checkout for an invoice that's already resolved.
        if invoice.paid_at.is_some() || invoice.voided_at.is_some() {
            return Err(anyhow!("invoice is not open"));
        }

        // Reuse a still-valid pending session so repeated clicks land on one page.
        if let Some(existing) = query::get_checkout_for_invoice(&invoice.id).await?
            && now < existing.expires_at
        {
            if existing.settled_at.is_some() {
                return Err(anyhow!(
                    "a checkout has already been settled for this invoice"
                ));
            }

            return Ok(existing);
        }

        // Stripe returns the tenant to their account page on success or cancel.
        // The landing page reconciles the tenant, which now settles a paid
        // Checkout out of band, so the URL needs no per-invoice marker.
        let return_url = format!("{}/account", env::get().app_url);

        let (session_id, url, expires_at) = self
            .stripe
            .create_checkout_session(
                &tenant.stripe_customer_id,
                &invoice.id,
                invoice.amount,
                "usd",
                &return_url,
                &return_url,
            )
            .await?;

        command::insert_checkout(&invoice.id, &session_id, &url, expires_at)
            .await?
            .ok_or_else(|| anyhow!("failed to insert checkout"))
    }

    // --- Stripe utils ---

    /// Refresh stripe-related state for a tenant, returning the synced payment
    /// method id (the tenant's existing one on failure). Fails gracefully.
    pub async fn sync_stripe_customer(&self, tenant: &Tenant) -> Result<Option<String>> {
        match self.sync_stripe_payment_method(tenant).await {
            Ok(payment_method_id) => Ok(payment_method_id),
            Err(error) => {
                tracing::error!(tenant = %tenant.pubkey, error = ?error, "failed to sync payment method");
                Ok(tenant.stripe_payment_method_id.clone())
            }
        }
    }

    /// Refresh the cached Stripe payment method from Stripe so collection can charge
    /// it directly and the UI reflects cards added via the portal.
    async fn sync_stripe_payment_method(&self, tenant: &Tenant) -> Result<Option<String>> {
        let payment_method_id = self
            .stripe
            .get_saved_payment_method(&tenant.stripe_customer_id)
            .await?;

        if payment_method_id != tenant.stripe_payment_method_id {
            command::set_tenant_stripe_payment_method(&tenant.pubkey, &payment_method_id).await?;
        }

        Ok(payment_method_id)
    }
}

/// One tenant's monthly billing period containing some timestamp, anchored at
/// the tenant's `billing_anchor`. Half-open `[start, end)` so a moment at
/// exactly `end` belongs to the next period.
pub struct BillingPeriod {
    pub start: i64,
    pub end: i64,
}

impl BillingPeriod {
    /// The period containing `chrono::Utc::now()` for `tenant`. `None` when the
    /// tenant has no `billing_anchor` yet — i.e. no billable activity has been seen.
    pub fn current(tenant: &Tenant) -> Option<Self> {
        Self::at(tenant, chrono::Utc::now().timestamp())
    }

    /// The period containing `at` for `tenant`. `None` when the tenant has no
    /// `billing_anchor` yet — i.e. no billable activity has been seen.
    fn at(tenant: &Tenant, at: i64) -> Option<Self> {
        use chrono::{DateTime, Months, Utc};

        let anchor = tenant.billing_anchor?;
        let anchor_dt = DateTime::<Utc>::from_timestamp(anchor, 0).unwrap_or_default();

        // Walk forward in whole calendar months from the anchor until the next
        // step would pass `at`, so boundaries track months (28–31 days) rather
        // than a fixed span of seconds.
        let mut start = anchor_dt;
        let mut months = 1u32;
        while let Some(next) = anchor_dt.checked_add_months(Months::new(months)) {
            if next.timestamp() > at {
                break;
            }
            start = next;
            months += 1;
        }

        // Derive `end` from the anchor (the next walk boundary), not `start + 1
        // month`: month-add clamps to the last valid day, so re-adding from a
        // clamped `start` lands short of `anchor + months` and leaves non-tiling
        // gap days for day-29–31 anchors. `months` is already that next boundary.
        let end = anchor_dt
            .checked_add_months(Months::new(months))
            .unwrap_or(start);

        Some(Self {
            start: start.timestamp(),
            end: end.timestamp(),
        })
    }

    /// Fraction of this period still unused at `at`, in `[0.0, 1.0]`, for
    /// prorating a mid-period charge or credit. The remaining time is rounded to
    /// the nearest hour so proration tracks whole hours rather than exact seconds.
    fn fraction_remaining(&self, at: i64) -> f64 {
        const HOUR: i64 = 60 * 60;

        let len = (self.end - self.start) as f64;
        if len <= 0.0 {
            return 1.0;
        }
        let remaining = ((self.end - at) + HOUR / 2) / HOUR * HOUR;
        (remaining as f64 / len).clamp(0.0, 1.0)
    }

    /// Prorate a minor-unit `amount` by the fraction of this period remaining
    /// at `at`, rounded to the nearest unit.
    fn prorate(&self, amount: i64, at: i64) -> i64 {
        (amount as f64 * self.fraction_remaining(at)).round() as i64
    }
}
