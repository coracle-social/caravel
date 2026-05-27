use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::time::Duration;

use crate::bitcoin;
use crate::command;
use crate::db;
use crate::env;
use crate::models::{Activity, Bolt11, Invoice, InvoiceItem, Tenant};
use crate::query;
use crate::robot::Robot;
use crate::stripe::Stripe;
use crate::wallet::Wallet;

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
        let mut rx = db::subscribe();

        tokio::spawn({
            let billing = self.clone();
            async move { billing.poll().await }
        });

        if let Err(error) = self.reconcile_subscriptions("startup").await {
            tracing::error!(error = %error, "failed to reconcile subscriptions on startup");
        }

        loop {
            match rx.recv().await {
                Ok(activity) => {
                    if let Err(e) = self.handle_activity(&activity).await {
                        tracing::error!(error = %e, "billing handle_activity failed");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(missed = n, "billing lagged, reconciling all subscriptions");

                    if let Err(error) = self.reconcile_subscriptions("lagged").await {
                        tracing::error!(error = %error, "failed to reconcile after lag");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    async fn poll(&self) {
        let mut interval = tokio::time::interval(POLL_INTERVAL);

        loop {
            interval.tick().await;

            if let Err(error) = self.autogenerate_invoices().await {
                tracing::error!(error = %error, "billing poll failed");
            }
        }
    }

    async fn autogenerate_invoices(&self) -> Result<()> {
        let tenants = query::list_tenants().await?;

        tracing::info!(
            tenant_count = tenants.len(),
            "polling tenants for subscription renewal"
        );

        for tenant in tenants {
            if let Err(error) = self.autogenerate_invoice(&tenant).await {
                tracing::error!(
                    tenant = %tenant.pubkey,
                    error = ?error,
                    "failed to autogenerate invoice"
                );
            }
        }

        Ok(())
    }

    /// Poll entry point: generate the tenant's invoice for the current period
    /// (adding any due renewals) and, if one results, collect payment.
    async fn autogenerate_invoice(&self, tenant: &Tenant) -> Result<()> {
        if let Some(invoice) = self.generate_invoice(tenant).await? {
            self.attempt_payment(tenant, &invoice).await?;
        }

        Ok(())
    }

    async fn handle_activity(&self, activity: &Activity) -> Result<()> {
        let should_reconcile = matches!(
            activity.activity_type.as_str(),
            "create_relay" | "update_relay" | "activate_relay" | "deactivate_relay"
        );

        if should_reconcile
            && let Some(tenant) = query::get_tenant(&activity.tenant).await?
        {
            self.reconcile_subscription(&tenant).await?;
        }

        Ok(())
    }

    async fn reconcile_subscriptions(&self, source: &str) -> Result<()> {
        let tenants = query::list_tenants().await?;

        tracing::info!(
            source,
            tenant_count = tenants.len(),
            "reconciling all subscriptions"
        );

        for tenant in tenants {
            if let Err(error) = self.reconcile_subscription(&tenant).await {
                tracing::error!(
                    source,
                    tenant = %tenant.pubkey,
                    error = ?error,
                    "failed to reconcile subscription"
                );
            }
        }

        Ok(())
    }

    // --- Reconciliation, renewal, and on-demand billing ---

    async fn reconcile_subscription(&self, tenant: &Tenant) -> Result<()> {
        let mut tenant = tenant.clone();

        for activity in query::list_billable_activity_for_tenant(&tenant.pubkey).await? {
            if tenant.billing_anchor.is_none() {
                tenant.billing_anchor = Some(activity.created_at);
                command::set_tenant_billing_anchor(&tenant).await?;
            }

            self.reconcile_activity(&tenant, &activity).await?;
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
            Some(item) => command::insert_invoice_item_for_activity(&item, &activity.id).await,
            None => command::mark_activity_billed(&activity.id).await,
        }
    }

    /// A prorated charge (or credit, with `sign` = -1) for the relay's current
    /// plan. `None` for a missing relay or a free plan. Mid-period items don't
    /// stamp `period_start` — the renewal decides coverage from activity history.
    async fn make_prorated_item(
        &self,
        tenant: &Tenant,
        activity: &Activity,
        sign: i64,
        description: &str,
    ) -> Result<Option<InvoiceItem>> {
        let Some(relay) = query::get_relay(&activity.resource_id).await? else {
            return Ok(None);
        };
        let Some(plan) = query::get_plan(&relay.plan) else {
            return Ok(None);
        };
        if plan.amount <= 0 {
            return Ok(None);
        }

        let anchor = tenant
            .billing_anchor
            .ok_or_else(|| anyhow!("billing anchor must be set before building prorated items"))?;
        let fraction = period_fraction_remaining(anchor, activity.created_at);
        let amount = sign * prorate(plan.amount, fraction);

        Ok(Some(line_item(activity, &relay.id, plan.id, amount, description, None)))
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
            return Ok(None);
        };
        let Some(old_plan_id) =
            query::get_relay_plan_before(&activity.resource_id, activity.created_at).await?
        else {
            return Ok(None);
        };
        if old_plan_id == new_plan_id {
            return Ok(None);
        }
        let (Some(new_plan), Some(old_plan)) =
            (query::get_plan(new_plan_id), query::get_plan(&old_plan_id))
        else {
            return Ok(None);
        };

        let anchor = tenant
            .billing_anchor
            .ok_or_else(|| anyhow!("billing anchor must be set before building prorated items"))?;
        let fraction = period_fraction_remaining(anchor, activity.created_at);
        let amount = prorate(new_plan.amount, fraction) - prorate(old_plan.amount, fraction);
        if amount == 0 {
            return Ok(None);
        }

        let description = format!("Plan changed from {} to {}", old_plan.name, new_plan.name);
        Ok(Some(line_item(
            activity,
            &activity.resource_id,
            new_plan.id,
            amount,
            &description,
            None,
        )))
    }

    /// Reconcile pending activity, add this period's renewals for any relay due,
    /// and claim everything outstanding onto an invoice. Shared by the poll and
    /// the on-demand invoice endpoint — safe to call either way: renewals are
    /// per-relay idempotent. No payment is attempted here; callers that want
    /// auto-pay do it on the returned invoice. `None` when nothing is owed.
    pub async fn generate_invoice(&self, tenant: &Tenant) -> Result<Option<Invoice>> {
        self.reconcile_subscription(tenant).await?;

        // reconcile may have just set the anchor (first activity); re-read it.
        let Some(tenant) = query::get_tenant(&tenant.pubkey).await? else {
            return Ok(None);
        };
        let Some(anchor) = tenant.billing_anchor else {
            return Ok(None);
        };

        let now = chrono::Utc::now().timestamp();
        let period_start = period_start_at(anchor, now);
        let period_end = add_one_month(period_start);

        self.renew_period(&tenant, period_start).await?;
        self.claim_outstanding(&tenant, period_start, period_end).await
    }

    /// Charge a full-period renewal for every relay that was active on a paid plan
    /// as of `period_start`, reconstructing that state from the activity log
    /// (status from create/activate/deactivate, plan from create/update). Per-relay
    /// idempotent via `period_start`, so calling it on every generation can't
    /// renew a relay twice; a relay created/activated *within* the period isn't
    /// active before the boundary, so it's covered by its own prorated charge.
    async fn renew_period(&self, tenant: &Tenant, period_start: i64) -> Result<()> {
        let activities = query::list_relay_activity_before(&tenant.pubkey, period_start).await?;

        let mut renewal_items = Vec::new();
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
            renewal_items.push(InvoiceItem {
                id: uuid::Uuid::new_v4().to_string(),
                invoice_id: None,
                activity_id: None,
                tenant_pubkey: tenant.pubkey.clone(),
                relay_id,
                plan: plan.id,
                amount: plan.amount,
                description: "Subscription renewal".to_string(),
                created_at: period_start,
                period_start: Some(period_start),
            });
        }

        command::create_renewal_items(&renewal_items).await
    }

    /// Claim the tenant's outstanding items onto a fresh invoice if they net
    /// positive; `None` when nothing is owed (a net credit stays outstanding and
    /// carries to the next positive invoice).
    async fn claim_outstanding(
        &self,
        tenant: &Tenant,
        period_start: i64,
        period_end: i64,
    ) -> Result<Option<Invoice>> {
        let invoice_id = uuid::Uuid::new_v4().to_string();
        command::claim_outstanding_into_invoice(
            &invoice_id,
            &tenant.pubkey,
            period_start,
            period_end,
        )
        .await
    }

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

/// The start of the billing period containing `now`, for monthly periods
/// anchored at `anchor`. Steps forward in whole calendar months so boundaries
/// track months (28–31 days) rather than a fixed span of seconds.
fn period_start_at(anchor: i64, now: i64) -> i64 {
    use chrono::{DateTime, Months, Utc};

    let anchor_dt = DateTime::<Utc>::from_timestamp(anchor, 0).unwrap_or_default();

    let mut start = anchor_dt;
    let mut months = 1u32;
    while let Some(next) = anchor_dt.checked_add_months(Months::new(months)) {
        if next.timestamp() > now {
            break;
        }
        start = next;
        months += 1;
    }

    start.timestamp()
}

/// One calendar month after `ts` (a unix timestamp), falling back to `ts` if the
/// shifted date can't be represented.
fn add_one_month(ts: i64) -> i64 {
    use chrono::{DateTime, Months, Utc};

    DateTime::<Utc>::from_timestamp(ts, 0)
        .and_then(|dt| dt.checked_add_months(Months::new(1)))
        .map(|dt| dt.timestamp())
        .unwrap_or(ts)
}

/// Fraction of the current billing period still unused at `at`, in `[0.0, 1.0]`,
/// for prorating a mid-period charge or credit. With no billing anchor yet the
/// period is only just beginning, so the whole period remains (full price).
fn period_fraction_remaining(billing_anchor: i64, at: i64) -> f64 {
    let period_start = period_start_at(billing_anchor, at);
    let period_end = add_one_month(period_start);
    let period_len = (period_end - period_start) as f64;
    if period_len <= 0.0 {
        return 1.0;
    }

    (((period_end - at) as f64) / period_len).clamp(0.0, 1.0)
}

/// Prorate a minor-unit `amount` by `fraction`, rounded to the nearest unit.
fn prorate(amount: i64, fraction: f64) -> i64 {
    (amount as f64 * fraction).round() as i64
}

/// Build an outstanding (unassigned, `invoice_id = None`) line item from a
/// reconciled activity. `period_start` is `Some` only for coverage charges
/// (creation/activation), which mark the relay-period as paid.
fn line_item(
    activity: &Activity,
    relay_id: &str,
    plan: String,
    amount: i64,
    description: &str,
    period_start: Option<i64>,
) -> InvoiceItem {
    InvoiceItem {
        id: uuid::Uuid::new_v4().to_string(),
        invoice_id: None,
        activity_id: Some(activity.id.clone()),
        tenant_pubkey: activity.tenant.clone(),
        relay_id: relay_id.to_string(),
        plan,
        amount,
        description: description.to_string(),
        created_at: activity.created_at,
        period_start,
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
