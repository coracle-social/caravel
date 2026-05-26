use anyhow::{Result, anyhow};
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

    /// If `tenant`'s subscription has rolled into a new billing period, claim it by
    /// atomically recording an `autogenerate_invoice` activity, then turn that into an invoice.
    async fn autogenerate_invoice(&self, tenant: &Tenant) -> Result<()> {
        // A subscription only exists once a billing anchor is set; until then
        // there is no schedule to renew against.
        let Some(billing_anchor) = tenant.billing_anchor else {
            return Ok(());
        };

        let now = chrono::Utc::now().timestamp();
        let period_start = period_start_at(billing_anchor, now);

        command::try_autogenerate_invoice(&tenant.pubkey, period_start).await?;

        Ok(())
    }

    async fn handle_activity(&self, activity: &Activity) -> Result<()> {
        let should_sync = matches!(
            activity.activity_type.as_str(),
            "create_relay" | "update_relay" | "activate_relay" | "deactivate_relay" | "autogenerate_invoice"
        );

        if should_sync
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

    // --- Invoice generation and autopayment ---

    /// Scan a tenant's activity for changes not yet reflected in an invoice and,
    /// if there are any, create an invoice with the corresponding line items and
    /// attempt to collect payment.
    async fn reconcile_subscription(&self, tenant: &Tenant) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        let invoice_id = uuid::Uuid::new_v4().to_string();

        let activities = query::list_billable_activity_for_tenant(&tenant.pubkey).await?;
        let billed_activity_ids: Vec<String> = activities.iter().map(|a| a.id.clone()).collect();

        let mut invoice_items: Vec<InvoiceItem> = Vec::new();

        for activity in &activities {
            // TODO: this is gross
            let relay = if activity.resource_type == "relay" {
                query::get_relay(&activity.resource_id).await?
            } else {
                None
            };

            match activity.activity_type.as_str() {
                "create_relay" => {
                    if let Some(relay) = &relay
                        && let Some(plan) = query::get_plan(&relay.plan)
                        && plan.amount > 0
                    {
                        // TODO: prorate amount based on billing anchor
                        invoice_items.push(InvoiceItem {
                            id: uuid::Uuid::new_v4().to_string(),
                            invoice_id: invoice_id.clone(),
                            activity_id: activity.id.clone(),
                            tenant_pubkey: tenant.pubkey.clone(),
                            relay_id: activity.resource_id.clone(),
                            plan: plan.id,
                            amount: plan.amount,
                            description: "New relay created".to_string(),
                            created_at: activity.created_at,
                        });
                    }
                }
                "update_relay" => {
                    // TODO: refund/charge prorated amount
                }
                "activate_relay" => {
                    // TODO: charge prorated amount
                }
                "deactivate_relay" => {
                    // TODO: refund prorated amount
                }
                "autogenerate_invoice" => {
                    // TODO: we're at the beginning of a new period, add invoice
                    // items for all active/paid relays
                }
                _ => {}
            }
        }

        // No line items (e.g. only free-plan or not-yet-prorated changes): still
        // stamp the activities billed so a recovery pass doesn't re-scan them.
        if invoice_items.is_empty() {
            command::mark_activities_billed(&billed_activity_ids).await?;
            return Ok(());
        }

        let period_start = invoice_items
            .iter()
            .map(|item| item.created_at)
            .max()
            .unwrap_or(now);
        let period_end = add_one_month(period_start);

        let invoice = command::create_invoice(
            &invoice_id,
            &tenant.pubkey,
            period_start,
            period_end,
            &invoice_items,
            &billed_activity_ids,
            tenant.billing_anchor.is_none().then_some(now),
        )
        .await?;

        self.attempt_payment(tenant, &invoice).await?;

        Ok(())
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
