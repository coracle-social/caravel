use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::time::Duration;

use crate::bitcoin;
use crate::command;
use crate::env;
use crate::models::{Activity, Bolt11, Invoice, InvoiceItem, Tenant};
use crate::query;
use crate::robot::Robot;
use crate::stripe::Stripe;
use crate::wallet::Wallet;

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

        tracing::info!(tenant_count = tenants.len(), "reconciling all subscriptions");

        for tenant in tenants {
            if let Err(error) = self.reconcile_subscription(&tenant).await {
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

    /// Lists billable activity, setting the tenant's billing anchor to the first
    /// activity in the process. Generates an invoice for the current period if due
    /// for renewal or any billable activities have occurred. Attempts payment if
    /// an invoice is generated.
    pub async fn reconcile_subscription(&self, tenant: &Tenant) -> Result<()> {
        let mut tenant = tenant.clone();

        // Reconcile all activity, setting the tenant's billing anchor on the first
        // positive-balance line item if not already set.
        for activity in query::list_billable_activity_for_tenant(&tenant.pubkey).await? {
            if tenant.billing_anchor.is_none() {
                tenant.billing_anchor = Some(activity.created_at);
                command::set_tenant_billing_anchor(&tenant).await?;
            }

            let invoice_item = self.reconcile_activity(&tenant, &activity).await?;
        }

        // If the tenant has no billing anchor, they have nothing to bill
        let Some(period) = BillingPeriod::current(&tenant) else {
            return Ok(());
        };

        // If tenant is due for renewal, bill any active relays.
        if tenant.renewed_at.is_none_or(|at| at < period.start) {
            self.reconcile_renewal(&tenant, &period).await?;
        }

        // Create the invoice, but only if non-zero and attempt payment
        if let Some(invoice) = command::create_invoice(&tenant, &period).await? {
            self.attempt_payment(&tenant, &invoice).await?;
        };

        Ok(())
    }

    /// Reconcile one activity into the ledger: build its line item (if any) and
    /// persist it with the activity's billed marker. Activities that produce no
    /// item (e.g. free-plan changes) are still marked billed so they aren't
    /// re-scanned.
    async fn reconcile_activity(&self, tenant: &Tenant, activity: &Activity) -> Result<Option<InvoiceItem>> {
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
            Some(ref item) => command::insert_invoice_item_for_activity(&item, &activity.id).await?,
            None => command::mark_activity_billed(&activity.id).await?,
        }

        Ok(invoice_item)
    }

    /// A prorated charge (or credit, with `sign` = -1) for the relay's current
    /// plan, covering the fraction of the period remaining at the activity.
    /// `None` for a missing relay or a free plan.
    async fn make_prorated_item(
        &self,
        tenant: &Tenant,
        activity: &Activity,
        sign: i64,
        description: &str,
    ) -> Result<Option<InvoiceItem>> {
        let Some(relay) = query::get_relay(&activity.resource_id).await? else {
            return anyhow!("activity resource was not a valid relay");
        };
        let Some(plan) = query::get_plan(&relay.plan) else {
            return anyhow!("activity plan was not a valid plan");
        };
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
            tenant_pubkey: activity.tenant.clone(),
            relay_id: activity.resource_id.clone(),
            plan: plan.id,
            amount,
            description: description.to_string(),
            created_at: activity.created_at,
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
        let Some(new_plan_id) = activity.plan_id.as_deref() else {
            return anyhow!("activity plan was not a valid plan");
        };
        let Some(old_plan_id) =
            query::get_relay_plan_before(&activity.resource_id, activity.created_at).await?
        else {
            return anyhow!("no previous plan found for relay update activity");
        };
        if old_plan_id == new_plan_id {
            return Ok(None);
        }
        let Some(new_plan) = query::get_plan(new_plan_id) else {
            return anyhow!("new plan is an invalid plan");
        };
        let Some(old_plan) = query::get_plan(old_plan_id) else {
            return anyhow!("old plan is an invalid plan");
        };

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
            tenant_pubkey: activity.tenant.clone(),
            relay_id: activity.resource_id.clone(),
            plan: new_plan.id,
            amount,
            description,
            created_at: activity.created_at,
        }))
    }

    /// Charge a full-period renewal for every relay that was active on a paid plan
    /// as of `period.start`, reconstructing that state from the activity log
    /// (status from create/activate/deactivate, plan from create/update).
    /// Idempotent per period via the tenant's `renewed_at` marker, so calling it
    /// on every generation can't renew twice; a relay created/activated *within*
    /// the period isn't active before the boundary, so it's covered by its own
    /// prorated charge instead.
    async fn reconcile_renewal(&self, tenant: &Tenant, period: &BillingPeriod) -> Result<()> {
        let activities = query::list_relay_activity_before(&tenant.pubkey, period.start).await?;

        let mut line_items = Vec::new();
        for (relay_id, state) in relay_states(&activities) {
            if !state.active {
                continue;
            }
            let Some(plan) = state.plan.and_then(|id| query::get_plan(&id)) else {
                continue;
            };
            if plan.amount <= 0 {
                continue;
            }
            line_items.push(InvoiceItem {
                id: uuid::Uuid::new_v4().to_string(),
                invoice_id: None,
                activity_id: None,
                tenant_pubkey: tenant.pubkey.clone(),
                relay_id,
                plan: plan.id,
                amount: plan.amount,
                description: "Subscription renewal".to_string(),
                created_at: period.start,
            });
        }

        // Inserts the items and advances `renewed_at` to `period.start` in one
        // transaction (idempotent via an in-tx guard), so a re-tick is a no-op.
        command::insert_invoice_items_for_renewal(&line_items, period).await
    }

    // --- Payments ---

    pub async fn attempt_payment(&self, tenant: &Tenant, invoice: &Invoice) -> Result<()> {
        let mut error_message: Option<String> = None;

        // 1. NWC auto-pay: if the tenant has configured an nwc_url, try it first.
        if !tenant.nwc_url.is_empty() {
            match self.attempt_payment_using_nwc(tenant, invoice).await {
                Ok(()) => return Ok(()),
                Err(e) => error_message = Some(format!("{e}")),
            }
        }

        // 2. Payment method on file: if the tenant has one saved, charge it via Stripe.
        if let Some(payment_method) =
            self.stripe.get_saved_payment_method(&tenant.stripe_customer_id).await?
        {
            match self
                .attempt_payment_using_stripe(tenant, invoice, &payment_method)
                .await
            {
                Ok(()) => return Ok(()),
                Err(e) => error_message = Some(format!("{e}")),
            }
        }

        // 3. Manual payment: DM a link to the in-app payment page for this invoice.
        let summary = error_message.as_deref().and_then(summarize_error_message);
        if let Err(e) = self.attempt_payment_using_dm(tenant, invoice, summary).await {
            tracing::error!(
                tenant = %tenant.pubkey,
                error = %e,
                "failed to send manual payment DM"
            );
        }

        Ok(())
    }

    async fn attempt_payment_using_nwc(&self, tenant: &Tenant, invoice: &Invoice) -> Result<()> {
        let nwc_url = env::get().decrypt(&tenant.nwc_url)?;
        let tenant_wallet = Wallet::from_url(&nwc_url)?;
        let bolt11 = self.ensure_bolt11(&invoice.id).await?;

        match tenant_wallet.pay_invoice(bolt11.lnbc.clone()).await {
            Ok(()) => {
                command::clear_tenant_nwc_error(&tenant.pubkey).await?;
                command::mark_bolt11_settled(&bolt11.id).await?;
                command::mark_invoice_paid(&invoice.id, "nwc").await
            }
            Err(pay_error) => {
                // The pay request errored, but the invoice may have been paid out of band.
                if self.wallet.is_settled(&bolt11.lnbc).await.unwrap_or(false) {
                    command::mark_bolt11_settled(&bolt11.id).await?;
                    command::mark_invoice_paid(&invoice.id, "oob").await
                } else {
                    Err(pay_error)
                }
            }
        }
    }

    async fn attempt_payment_using_stripe(
        &self,
        tenant: &Tenant,
        invoice: &Invoice,
        payment_method_id: &str,
    ) -> Result<()> {
        let amount = self.get_invoice_amount(&invoice.id).await?;

        // A decline or an off-session authentication demand comes back as Err, so
        // the cascade falls back to the manual DM.
        let intent_id = self
            .stripe
            .create_payment_intent(
                &tenant.stripe_customer_id,
                payment_method_id,
                &invoice.id,
                amount,
                "usd",
            )
            .await?;

        command::insert_intent(&intent_id, &invoice.id).await?;
        command::mark_invoice_paid(&invoice.id, "stripe").await
    }

    async fn attempt_payment_using_dm(
        &self,
        tenant: &Tenant,
        invoice: &Invoice,
        error_message: Option<String>,
    ) -> Result<()> {
        let invoice_id = &invoice.id;
        let url_base = &env::get().app_url;
        let payment_url = format!("{url_base}/account?invoice={invoice_id}");
        let base = format!("{MANUAL_PAYMENT_DM}\n\n{payment_url}");
        let dm_message = match error_message {
            Some(error_message) if !error_message.is_empty() => {
                format!("{base}\n\n{USER_ERROR_PREFIX} {error_message}")
            }
            _ => base,
        };

        self.robot.send_dm(&tenant.pubkey, &dm_message).await
    }

    // --- Invoice utils ---

    pub async fn get_invoice_amount(&self, invoice_id: &str) -> Result<i64> {
        let invoice_items = query::get_invoice_items_for_invoice(invoice_id).await?;

        Ok(invoice_items.iter().map(|item| item.amount).sum())
    }

    // --- Bolt11 utils ---

    pub async fn ensure_bolt11(&self, invoice_id: &str) -> Result<Bolt11> {
        let now = chrono::Utc::now().timestamp();

        if let Some(existing) = query::get_bolt11_for_invoice(invoice_id).await?
            && (existing.settled_at.is_none() || now < existing.expires_at)
        {
            return Ok(existing);
        }

        let expiry: i64 = 3600;
        let info = "Relay subscription payment";
        let amount = self.get_invoice_amount(invoice_id).await?;
        let msats = bitcoin::fiat_to_msats(amount, "usd").await?;
        let lnbc = self.wallet.make_invoice(msats, info, expiry as u64).await?;

        command::insert_bolt11(invoice_id, &lnbc, msats as i64, now + expiry)
            .await?
            .ok_or_else(|| anyhow!("failed to insert bolt11"))
    }

    /// Catch an out-of-band payment we never recorded — e.g. the user paid the
    /// invoice but the frontend failed to notify us. If the invoice's bolt11 has
    /// settled on the robot wallet, mark it paid and return the refreshed record;
    /// otherwise return it unchanged. Meant to run before presenting a payable
    /// invoice so we never hand back one that's already been paid.
    pub async fn ensure_and_reconcile_bolt11(&self, invoice_id: &str) -> Result<Bolt11> {
        let bolt11 = self.ensure_bolt11(invoice_id).await?;

        if bolt11.settled_at.is_none() && self.wallet.is_settled(&bolt11.lnbc).await? {
            command::mark_bolt11_settled(&bolt11.id).await?;

            // Re-fetch so the caller sees that it's been settled.
            Ok(query::get_bolt11(&bolt11.id).await?.unwrap_or(bolt11))
        } else {
            Ok(bolt11)
        }
    }
}

const POLL_INTERVAL: Duration = Duration::from_secs(60 * 60);
const MANUAL_PAYMENT_DM: &str = "Payment is due for your relay subscription. Open the link below to review the invoice and pay by Lightning or card:";
const USER_ERROR_PREFIX: &str = "NWC auto-payment failed:";
const USER_ERROR_MAX_CHARS: usize = 240;

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
    fn current(tenant: &Tenant) -> Option<Self> {
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

        let end = start.checked_add_months(Months::new(1)).unwrap_or(start);

        Some(Self {
            start: start.timestamp(),
            end: end.timestamp(),
        })
    }

    /// Fraction of this period still unused at `at`, in `[0.0, 1.0]`, for
    /// prorating a mid-period charge or credit.
    fn fraction_remaining(&self, at: i64) -> f64 {
        let len = (self.end - self.start) as f64;
        if len <= 0.0 {
            return 1.0;
        }
        (((self.end - at) as f64) / len).clamp(0.0, 1.0)
    }

    /// Prorate a minor-unit `amount` by the fraction of this period remaining
    /// at `at`, rounded to the nearest unit.
    fn prorate(&self, amount: i64, at: i64) -> i64 {
        (amount as f64 * self.fraction_remaining(at)).round() as i64
    }
}

/// A relay's billing-relevant state at a point in time, reconstructed by folding
/// its activity log.
#[derive(Default)]
struct RelayState {
    active: bool,
    plan: Option<String>,
}

/// Fold relay activities (which must be oldest-first) into each relay's
/// `(active, plan)` state. `create`/`activate`/`deactivate` drive status;
/// `create`/`update` carry the plan via `plan_id`. Feed it activities up to a
/// cutoff to get each relay's state as of that moment (e.g. the period boundary).
fn relay_states(activities: &[Activity]) -> HashMap<String, RelayState> {
    let mut states: HashMap<String, RelayState> = HashMap::new();

    for activity in activities {
        let state = states.entry(activity.resource_id.clone()).or_default();
        match activity.activity_type.as_str() {
            "create_relay" => {
                state.active = true;
                state.plan = activity.plan_id.clone();
            }
            "update_relay" => {
                if activity.plan_id.is_some() {
                    state.plan = activity.plan_id.clone();
                }
            }
            "activate_relay" => state.active = true,
            "deactivate_relay" => state.active = false,
            _ => {}
        }
    }

    states
}

fn summarize_error_message(error: &str) -> Option<String> {
    let normalized = error.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return None;
    }

    if normalized.chars().count() <= USER_ERROR_MAX_CHARS {
        return Some(normalized);
    }

    let prefix_len = USER_ERROR_MAX_CHARS.saturating_sub(3);
    let mut truncated = normalized.chars().take(prefix_len).collect::<String>();
    truncated.push_str("...");
    Some(truncated)
}
