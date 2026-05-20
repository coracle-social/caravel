use anyhow::{Result, anyhow};
use std::collections::BTreeMap;

use crate::bitcoin;
use crate::command::Command;
use crate::env::Env;
use crate::models::{Activity, RELAY_STATUS_ACTIVE, RELAY_STATUS_DELINQUENT};
use crate::query::Query;
use crate::robot::Robot;
use crate::stripe::{Stripe, StripeInvoice};
use crate::wallet::Wallet;

const MANUAL_LIGHTNING_PAYMENT_DM: &str = "Payment is due for your relay subscription. Please visit the application to complete a manual Lightning payment.";
const NWC_ERROR_DM_PREFIX: &str = "NWC auto-payment failed:";
const NWC_ERROR_DM_MAX_CHARS: usize = 240;
const LIGHTNING_INVOICE_DESCRIPTION: &str = "Relay subscription payment";

enum NwcInvoicePaymentOutcome {
    Paid,
    Fallback(anyhow::Error),
    Pending(anyhow::Error),
}

#[derive(Clone)]
pub struct Billing {
    stripe: Stripe,
    wallet: Wallet,
    env: Env,
    query: Query,
    command: Command,
    robot: Robot,
}

impl Billing {
    pub fn new(query: Query, command: Command, robot: Robot, env: &Env) -> Self {
        Self {
            stripe: Stripe::new(
                env.stripe_secret_key.clone(),
                env.stripe_webhook_secret.clone(),
            ),
            wallet: Wallet::from_url(&env.robot_wallet).expect("invalid ROBOT_WALLET"),
            env: env.clone(),
            query,
            command,
            robot,
        }
    }

    pub async fn start(self) {
        let mut rx = self.command.notify.subscribe();

        if let Err(error) = self.reconcile_relay_subscriptions("startup").await {
            tracing::error!(error = %error, "failed to reconcile relay billing state on startup");
        }

        loop {
            match rx.recv().await {
                Ok(activity) => {
                    if let Err(e) = self.handle_activity(&activity).await {
                        tracing::error!(error = %e, "billing handle_activity failed");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(missed = n, "billing lagged");

                    if let Err(error) = self.reconcile_relay_subscriptions("lagged").await {
                        tracing::error!(error = %error, "failed to reconcile relay billing state after lag");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    async fn reconcile_relay_subscriptions(&self, source: &str) -> Result<()> {
        let tenants = self.query.list_tenants().await?;

        if tenants.is_empty() {
            return Ok(());
        }

        tracing::info!(
            source,
            tenant_count = tenants.len(),
            "reconciling relay billing state"
        );

        for tenant in tenants {
            if let Err(error) = self.sync_tenant_subscription(&tenant.pubkey).await {
                tracing::error!(
                    source,
                    tenant = %tenant.pubkey,
                    error = ?error,
                    "failed to reconcile relay billing state"
                );
            }
        }

        Ok(())
    }

    async fn handle_activity(&self, activity: &Activity) -> Result<()> {
        let needs_billing_sync = matches!(
            activity.activity_type.as_str(),
            "create_relay"
                | "update_relay"
                | "activate_relay"
                | "deactivate_relay"
                | "fail_relay_sync"
                | "complete_relay_sync"
        );

        if needs_billing_sync
            && let Some(relay) = self.query.get_relay(&activity.resource_id).await?
        {
            self.sync_tenant_subscription(&relay.tenant).await?;
        }

        Ok(())
    }

    /// Reconciles a tenant's single Stripe subscription with the set of relays that
    /// should be billed.
    ///
    /// Stripe forbids two subscription items on the same subscription from sharing a
    /// price, so billing is modeled as one subscription item per plan (price) with
    /// `quantity` equal to the number of the tenant's `active` relays on that plan.
    /// Every such relay's `stripe_subscription_item_id` points at the shared item for
    /// its plan; relays that aren't billed (free, inactive, delinquent) have it
    /// cleared. Must be idempotent.
    async fn sync_tenant_subscription(&self, tenant_pubkey: &str) -> Result<()> {
        let Some(mut tenant) = self.query.get_tenant(tenant_pubkey).await? else {
            return Ok(());
        };

        let relays = self.query.list_relays_for_tenant(tenant_pubkey).await?;

        // Desired billed state: price id -> quantity, plus which relays map to which price.
        let mut desired: BTreeMap<String, i64> = BTreeMap::new();
        let mut relay_price: BTreeMap<String, String> = BTreeMap::new();
        for relay in &relays {
            if relay.status != RELAY_STATUS_ACTIVE {
                continue;
            }
            let Some(plan) = self.query.get_plan(&relay.plan) else {
                tracing::warn!(relay = %relay.id, plan = %relay.plan, "active relay on unknown plan; not billed");
                continue;
            };
            let Some(price_id) = plan.stripe_price_id else {
                continue; // free plan: nothing to bill
            };
            if price_id.trim().is_empty() {
                tracing::warn!(relay = %relay.id, plan = %relay.plan, "active relay on a paid plan with no configured Stripe price id; not billed");
                continue;
            }
            *desired.entry(price_id.clone()).or_insert(0) += 1;
            relay_price.insert(relay.id.clone(), price_id);
        }

        // Resolve the live subscription, dropping a stale reference to one that no
        // longer exists or has been canceled.
        let subscription = match tenant.stripe_subscription_id.as_deref() {
            Some(subscription_id) => match self.stripe.get_subscription(subscription_id).await? {
                Some(sub) if !matches!(sub.status.as_str(), "canceled" | "incomplete_expired") => {
                    Some(sub)
                }
                _ => {
                    self.command
                        .clear_tenant_subscription(tenant_pubkey)
                        .await?;
                    tenant.stripe_subscription_id = None;
                    None
                }
            },
            None => None,
        };

        // No relays to bill: tear everything down.
        if desired.is_empty() {
            if let Some(ref subscription_id) = tenant.stripe_subscription_id {
                self.stripe.cancel_subscription(subscription_id).await?;
                self.command
                    .clear_tenant_subscription(tenant_pubkey)
                    .await?;
            }
            for relay in &relays {
                if relay.stripe_subscription_item_id.is_some() {
                    self.command
                        .clear_relay_subscription_item(&relay.id)
                        .await?;
                }
            }
            return Ok(());
        }

        // Bring the subscription's items in line with `desired`. `price_to_item` ends
        // up mapping every desired price to its (possibly newly created) item id.
        let mut price_to_item: BTreeMap<String, String> = BTreeMap::new();
        let mut downgraded = false;

        match subscription {
            None => {
                let sub = self
                    .stripe
                    .create_subscription(&tenant.stripe_customer_id, &desired)
                    .await?;
                self.command
                    .set_tenant_subscription(tenant_pubkey, &sub.id)
                    .await?;
                for item in sub.items {
                    price_to_item.insert(item.price.id, item.id);
                }
                tenant.stripe_subscription_id = Some(sub.id);
            }
            Some(sub) => {
                let subscription_id = sub.id;

                // price id -> (item id, quantity) for items currently on the subscription.
                let mut current: BTreeMap<String, (String, i64)> = BTreeMap::new();
                for item in sub.items {
                    current.insert(item.price.id, (item.id, item.quantity));
                }

                for (price_id, &quantity) in &desired {
                    if let Some((item_id, current_quantity)) = current.remove(price_id) {
                        if current_quantity != quantity {
                            if quantity < current_quantity {
                                downgraded = true;
                            }
                            self.stripe
                                .set_subscription_item_quantity(&item_id, quantity)
                                .await?;
                        }
                        price_to_item.insert(price_id.clone(), item_id);
                    } else {
                        let item_id = self
                            .stripe
                            .create_subscription_item(&subscription_id, price_id, quantity)
                            .await?;
                        price_to_item.insert(price_id.clone(), item_id);
                    }
                }

                // Items for plans no relay is on anymore.
                for (_, (item_id, _)) in current {
                    downgraded = true;
                    self.stripe.delete_subscription_item(&item_id).await?;
                }
            }
        }

        // Point each relay at the shared item for its plan (or clear it if unbilled).
        for relay in &relays {
            match relay_price.get(&relay.id) {
                Some(price_id) => {
                    let item_id = price_to_item
                        .get(price_id)
                        .ok_or_else(|| anyhow!("missing subscription item for price {price_id}"))?;
                    if relay.stripe_subscription_item_id.as_deref() != Some(item_id.as_str()) {
                        self.command
                            .set_relay_subscription_item(&relay.id, item_id)
                            .await?;
                    }
                }
                None => {
                    if relay.stripe_subscription_item_id.is_some() {
                        self.command
                            .clear_relay_subscription_item(&relay.id)
                            .await?;
                    }
                }
            }
        }

        if downgraded {
            self.validate_downgrade_proration(&tenant, "tenant-subscription-sync")
                .await;
        }

        Ok(())
    }

    async fn existing_invoice_nwc_payment_outcome(
        &self,
        invoice_id: &str,
    ) -> Result<Option<NwcInvoicePaymentOutcome>> {
        let state = self.query.get_invoice_nwc_payment_state(invoice_id).await?;
        match state.as_deref() {
            Some("paid") => Ok(Some(NwcInvoicePaymentOutcome::Paid)),
            Some("pending") => Ok(Some(NwcInvoicePaymentOutcome::Pending(anyhow!(
                "invoice {invoice_id} has a pending NWC reconciliation; refusing to create a new Lightning charge"
            )))),
            Some(other) => Err(anyhow!(
                "unknown invoice_nwc_payment state '{other}' for invoice {invoice_id}"
            )),
            None => Ok(None),
        }
    }

    pub async fn handle_webhook(&self, payload: &str, signature: &str) -> Result<()> {
        let event = self.stripe.get_webhook_event(payload, signature)?;
        let obj = &event.data.object;

        match event.event_type.as_str() {
            "invoice.created" => {
                let customer = obj["customer"].as_str().unwrap_or_default();
                let amount_due = obj["amount_due"].as_i64().unwrap_or(0);
                let currency = obj["currency"].as_str().unwrap_or("usd");
                let invoice_id = obj["id"].as_str().unwrap_or_default();
                self.handle_invoice_created(customer, amount_due, currency, invoice_id)
                    .await?;
            }
            "invoice.paid" => {
                let customer = obj["customer"].as_str().unwrap_or_default();
                self.handle_invoice_paid(customer).await?;
            }
            "invoice.payment_failed" => {
                let customer = obj["customer"].as_str().unwrap_or_default();
                self.handle_invoice_payment_failed(customer).await?;
            }
            "invoice.overdue" => {
                let customer = obj["customer"].as_str().unwrap_or_default();
                self.handle_invoice_overdue(customer).await?;
            }
            "customer.subscription.updated" => {
                let customer = obj["customer"].as_str().unwrap_or_default();
                let status = obj["status"].as_str().unwrap_or_default();
                self.handle_subscription_updated(customer, status).await?;
            }
            "customer.subscription.deleted" => {
                let customer = obj["customer"].as_str().unwrap_or_default();
                self.handle_subscription_deleted(customer).await?;
            }
            "payment_method.attached" => {
                let customer = obj["customer"].as_str().unwrap_or_default();
                self.handle_payment_method_attached(customer).await?;
            }
            _ => {}
        }

        Ok(())
    }

    async fn handle_invoice_created(
        &self,
        stripe_customer_id: &str,
        amount_due: i64,
        currency: &str,
        invoice_id: &str,
    ) -> Result<()> {
        if amount_due == 0 {
            return Ok(());
        }

        let Some(tenant) = self
            .query
            .get_tenant_by_stripe_customer_id(stripe_customer_id)
            .await?
        else {
            return Ok(());
        };

        let mut nwc_error_for_dm: Option<String> = None;

        // 1. NWC auto-pay: if the tenant has a nwc_url
        if !tenant.nwc_url.is_empty() {
            let plain_nwc_url = self.env.decrypt(&tenant.nwc_url)?;
            match self
                .nwc_pay_invoice(
                    invoice_id,
                    &tenant.pubkey,
                    amount_due,
                    currency,
                    &plain_nwc_url,
                )
                .await?
            {
                NwcInvoicePaymentOutcome::Paid => {
                    self.mark_invoice_paid_out_of_band_after_nwc(invoice_id, &tenant.pubkey)
                        .await?;
                    return Ok(());
                }
                NwcInvoicePaymentOutcome::Fallback(e) => {
                    let error_msg = format!("{e}");
                    self.command
                        .set_tenant_nwc_error(&tenant.pubkey, &error_msg)
                        .await?;
                    tracing::warn!(
                        error = %e,
                        tenant_pubkey = %tenant.pubkey,
                        stripe_customer_id,
                        invoice_id,
                        "nwc auto-payment failed for invoice.created"
                    );
                    nwc_error_for_dm = summarize_nwc_error_for_dm(&error_msg);
                    // Fall through to next option
                }
                NwcInvoicePaymentOutcome::Pending(e) => {
                    let error_msg = format!("{e}");
                    self.command
                        .set_tenant_nwc_error(&tenant.pubkey, &error_msg)
                        .await?;
                    tracing::error!(
                        error = %e,
                        tenant_pubkey = %tenant.pubkey,
                        stripe_customer_id,
                        invoice_id,
                        "nwc auto-payment requires reconciliation before retry"
                    );
                    return Err(e);
                }
            }
        }

        // 2. Card on file: if the tenant has a payment method, Stripe charges automatically
        if self
            .stripe
            .has_payment_method(&tenant.stripe_customer_id)
            .await?
        {
            return Ok(());
        }

        // 3. Manual payment: send a DM
        let dm_message = manual_lightning_payment_dm(nwc_error_for_dm.as_deref());
        self.robot.send_dm(&tenant.pubkey, &dm_message).await?;

        Ok(())
    }

    async fn handle_invoice_paid(&self, stripe_customer_id: &str) -> Result<()> {
        let Some(tenant) = self
            .query
            .get_tenant_by_stripe_customer_id(stripe_customer_id)
            .await?
        else {
            return Ok(());
        };

        if tenant.past_due_at.is_some() {
            self.command.clear_tenant_past_due(&tenant.pubkey).await?;

            let relays = self.query.list_relays_for_tenant(&tenant.pubkey).await?;
            for relay in relays {
                if relay.status == RELAY_STATUS_DELINQUENT && self.query.is_paid_plan(&relay.plan) {
                    self.command.activate_relay(&relay).await?;
                }
            }
        }

        Ok(())
    }

    async fn handle_invoice_payment_failed(&self, stripe_customer_id: &str) -> Result<()> {
        let Some(tenant) = self
            .query
            .get_tenant_by_stripe_customer_id(stripe_customer_id)
            .await?
        else {
            return Ok(());
        };

        if tenant.past_due_at.is_none() {
            self.command.set_tenant_past_due(&tenant.pubkey).await?;
            self.robot
                .send_dm(
                    &tenant.pubkey,
                    "Your payment has failed. Your relays may be deactivated if not resolved within a week.",
                )
                .await?;
        }

        Ok(())
    }

    async fn handle_subscription_updated(
        &self,
        stripe_customer_id: &str,
        status: &str,
    ) -> Result<()> {
        if status != "canceled" && status != "unpaid" {
            return Ok(());
        }

        let Some(tenant) = self
            .query
            .get_tenant_by_stripe_customer_id(stripe_customer_id)
            .await?
        else {
            return Ok(());
        };

        self.command
            .clear_tenant_subscription(&tenant.pubkey)
            .await?;

        let relays = self.query.list_relays_for_tenant(&tenant.pubkey).await?;
        for relay in relays {
            if relay.status == RELAY_STATUS_ACTIVE && self.query.is_paid_plan(&relay.plan) {
                self.command.mark_relay_delinquent(&relay).await?;
            }
        }

        Ok(())
    }

    async fn handle_subscription_deleted(&self, stripe_customer_id: &str) -> Result<()> {
        let Some(tenant) = self
            .query
            .get_tenant_by_stripe_customer_id(stripe_customer_id)
            .await?
        else {
            return Ok(());
        };

        self.command
            .clear_tenant_subscription(&tenant.pubkey)
            .await?;

        Ok(())
    }

    async fn handle_invoice_overdue(&self, stripe_customer_id: &str) -> Result<()> {
        let Some(tenant) = self
            .query
            .get_tenant_by_stripe_customer_id(stripe_customer_id)
            .await?
        else {
            return Ok(());
        };

        let relays = self.query.list_relays_for_tenant(&tenant.pubkey).await?;
        for relay in relays {
            if relay.status == RELAY_STATUS_ACTIVE && self.query.is_paid_plan(&relay.plan) {
                self.command.mark_relay_delinquent(&relay).await?;
            }
        }

        self.robot
            .send_dm(
                &tenant.pubkey,
                "Your paid relays have been deactivated due to non-payment.",
            )
            .await?;

        Ok(())
    }

    async fn handle_payment_method_attached(&self, stripe_customer_id: &str) -> Result<()> {
        if stripe_customer_id.is_empty() {
            return Ok(());
        }

        let Some(tenant) = self
            .query
            .get_tenant_by_stripe_customer_id(stripe_customer_id)
            .await?
        else {
            return Ok(());
        };

        self.pay_outstanding_card_invoices(&tenant).await?;
        Ok(())
    }

    async fn validate_downgrade_proration(&self, tenant: &crate::models::Tenant, context: &str) {
        match self
            .stripe
            .preview_invoice(
                &tenant.stripe_customer_id,
                tenant.stripe_subscription_id.as_deref(),
            )
            .await
        {
            Ok(upcoming) => {
                let proration_lines = upcoming.lines.iter().filter(|line| line.proration).count();

                tracing::info!(
                    tenant_pubkey = %tenant.pubkey,
                    stripe_customer_id = %tenant.stripe_customer_id,
                    context,
                    proration_lines,
                    amount_due = upcoming.amount_due,
                    currency = %upcoming.currency,
                    "validated Stripe proration preview for downgrade"
                );

                if proration_lines == 0 {
                    tracing::warn!(
                        tenant_pubkey = %tenant.pubkey,
                        context,
                        "downgrade proration preview has no proration lines; verify in Stripe dashboard"
                    );
                }
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    tenant_pubkey = %tenant.pubkey,
                    context,
                    "failed to fetch downgrade proration preview"
                );
            }
        }
    }

    // --- Public API helpers ---

    /// Returns `Ok(None)` if Stripe has no such invoice; the route turns that into a 404.
    pub async fn get_invoice_with_tenant(
        &self,
        invoice_id: &str,
    ) -> Result<Option<(StripeInvoice, crate::models::Tenant)>> {
        let Some(invoice) = self.stripe.get_invoice(invoice_id).await? else {
            return Ok(None);
        };
        let tenant = self
            .query
            .get_tenant_by_stripe_customer_id(&invoice.customer)
            .await?
            .ok_or_else(|| anyhow!("tenant not found for customer"))?;
        Ok(Some((invoice, tenant)))
    }

    pub async fn reconcile_manual_lightning_invoice(
        &self,
        invoice_id: &str,
        invoice: &StripeInvoice,
    ) -> Result<StripeInvoice> {
        self.reconcile_manual_lightning_invoice_if_settled(invoice_id, invoice)
            .await
    }

    pub async fn get_or_create_manual_lightning_bolt11(
        &self,
        invoice_id: &str,
        tenant_pubkey: &str,
        amount_due_minor: i64,
        currency: &str,
    ) -> Result<String> {
        if let Some(existing_bolt11) = self
            .query
            .get_invoice_manual_lightning_bolt11(invoice_id)
            .await?
        {
            return Ok(existing_bolt11);
        }

        let bolt11 = self.create_bolt11(amount_due_minor, currency).await?;

        if self
            .command
            .insert_manual_lightning_invoice_payment(invoice_id, tenant_pubkey, &bolt11)
            .await?
        {
            return Ok(bolt11);
        }

        self.query
            .get_invoice_manual_lightning_bolt11(invoice_id)
            .await?
            .ok_or_else(|| {
                anyhow!(
                    "manual lightning payment row missing after insert race for invoice {invoice_id}"
                )
            })
    }

    pub async fn stripe_create_customer(&self, tenant_pubkey: &str) -> Result<String> {
        let short_pubkey: String = tenant_pubkey.chars().take(8).collect();
        let display_name = self
            .robot
            .fetch_nostr_name(tenant_pubkey)
            .await
            .unwrap_or(short_pubkey);
        self.stripe
            .create_customer(tenant_pubkey, &display_name)
            .await
    }

    pub async fn stripe_list_invoices(&self, customer_id: &str) -> Result<Vec<StripeInvoice>> {
        self.stripe.list_invoices(customer_id).await
    }

    pub async fn stripe_create_portal_session(
        &self,
        customer_id: &str,
        return_url: Option<&str>,
    ) -> Result<String> {
        self.stripe
            .create_portal_session(customer_id, return_url)
            .await
    }

    pub async fn create_bolt11(&self, amount_due_minor: i64, currency: &str) -> Result<String> {
        let amount_msats = bitcoin::fiat_to_msats(amount_due_minor, currency).await?;
        self.wallet
            .make_invoice(amount_msats, LIGHTNING_INVOICE_DESCRIPTION)
            .await
    }

    pub async fn pay_outstanding_nwc_invoices(&self, tenant: &crate::models::Tenant) -> Result<()> {
        if tenant.nwc_url.is_empty() {
            return Ok(());
        }

        let plain_nwc_url = self.env.decrypt(&tenant.nwc_url)?;

        let invoices = self
            .stripe
            .list_invoices(&tenant.stripe_customer_id)
            .await?;

        for invoice in &invoices {
            if invoice.status != "open" || invoice.amount_due == 0 {
                continue;
            }
            let invoice_id = invoice.id.as_str();

            match self
                .nwc_pay_invoice(
                    invoice_id,
                    &tenant.pubkey,
                    invoice.amount_due,
                    &invoice.currency,
                    &plain_nwc_url,
                )
                .await?
            {
                NwcInvoicePaymentOutcome::Paid => {
                    if let Err(e) = self
                        .mark_invoice_paid_out_of_band_after_nwc(invoice_id, &tenant.pubkey)
                        .await
                    {
                        tracing::error!(
                            error = %e,
                            invoice_id,
                            "failed to mark invoice paid out of band"
                        );
                    }
                }
                NwcInvoicePaymentOutcome::Fallback(e) => {
                    let error_msg = format!("{e}");
                    tracing::error!(
                        error = %e,
                        invoice_id,
                        "nwc payment failed for outstanding invoice"
                    );
                    let _ = self
                        .command
                        .set_tenant_nwc_error(&tenant.pubkey, &error_msg)
                        .await;
                }
                NwcInvoicePaymentOutcome::Pending(e) => {
                    let error_msg = format!("{e}");
                    tracing::error!(
                        error = %e,
                        invoice_id,
                        "outstanding invoice requires NWC reconciliation before retry"
                    );
                    let _ = self
                        .command
                        .set_tenant_nwc_error(&tenant.pubkey, &error_msg)
                        .await;
                }
            }
        }

        Ok(())
    }

    async fn pay_outstanding_card_invoices(&self, tenant: &crate::models::Tenant) -> Result<()> {
        if !self
            .stripe
            .has_payment_method(&tenant.stripe_customer_id)
            .await?
        {
            return Ok(());
        }

        let invoices = self
            .stripe
            .list_invoices(&tenant.stripe_customer_id)
            .await?;

        for invoice in &invoices {
            if invoice.status != "open" || invoice.amount_due == 0 {
                continue;
            }
            if let Err(error) = self.stripe.pay_invoice(&invoice.id).await {
                tracing::error!(
                    error = %error,
                    invoice_id = %invoice.id,
                    "failed to retry card payment for outstanding invoice"
                );
            }
        }

        Ok(())
    }

    // --- Lightning / NWC orchestration ---

    async fn mark_invoice_paid_out_of_band_after_nwc(
        &self,
        invoice_id: &str,
        tenant_pubkey: &str,
    ) -> Result<()> {
        self.stripe.pay_invoice_out_of_band(invoice_id).await?;
        self.command.clear_tenant_nwc_error(tenant_pubkey).await?;
        Ok(())
    }

    async fn reconcile_manual_lightning_invoice_if_settled(
        &self,
        invoice_id: &str,
        invoice: &StripeInvoice,
    ) -> Result<StripeInvoice> {
        if invoice.status != "open" {
            return Ok(invoice.clone());
        }

        let Some(bolt11) = self
            .query
            .get_invoice_manual_lightning_bolt11(invoice_id)
            .await?
        else {
            return Ok(invoice.clone());
        };

        let settled = match self.is_manual_lightning_invoice_settled(&bolt11).await {
            Ok(settled) => settled,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    invoice_id,
                    "failed to lookup manual lightning invoice settlement"
                );
                return Ok(invoice.clone());
            }
        };

        if !settled {
            return Ok(invoice.clone());
        }

        if let Err(error) = self.stripe.pay_invoice_out_of_band(invoice_id).await {
            tracing::warn!(
                error = %error,
                invoice_id,
                "failed to mark settled manual lightning invoice as paid_out_of_band"
            );
        }

        // The invoice existed when we called pay_invoice_out_of_band a moment ago;
        // if Stripe suddenly returns 404, fall back to the pre-reconcile snapshot
        // rather than failing the request.
        Ok(self
            .stripe
            .get_invoice(invoice_id)
            .await?
            .unwrap_or_else(|| invoice.clone()))
    }

    async fn is_manual_lightning_invoice_settled(&self, bolt11: &str) -> Result<bool> {
        self.wallet.is_settled(bolt11).await
    }

    /// Charges a Stripe invoice over Lightning: the system wallet issues a bolt11
    /// invoice for the fiat amount, the tenant's wallet pays it. A `pending` row in
    /// `invoice_nwc_payment` guards against double-charging across retries.
    async fn nwc_pay_invoice(
        &self,
        invoice_id: &str,
        tenant_pubkey: &str,
        amount_due_minor: i64,
        currency: &str,
        tenant_nwc_url: &str,
    ) -> Result<NwcInvoicePaymentOutcome> {
        if let Some(existing_outcome) = self
            .existing_invoice_nwc_payment_outcome(invoice_id)
            .await?
        {
            return Ok(existing_outcome);
        }

        let amount_msats = match bitcoin::fiat_to_msats(amount_due_minor, currency).await {
            Ok(msats) => msats,
            Err(error) => return Ok(NwcInvoicePaymentOutcome::Fallback(error)),
        };

        let bolt11 = match self
            .wallet
            .make_invoice(amount_msats, LIGHTNING_INVOICE_DESCRIPTION)
            .await
        {
            Ok(bolt11) => bolt11,
            Err(error) => return Ok(NwcInvoicePaymentOutcome::Fallback(error)),
        };

        let tenant_wallet = match Wallet::from_url(tenant_nwc_url) {
            Ok(wallet) => wallet,
            Err(error) => return Ok(NwcInvoicePaymentOutcome::Fallback(error)),
        };

        if !self
            .command
            .insert_pending_invoice_nwc_payment(invoice_id, tenant_pubkey)
            .await?
        {
            if let Some(existing_outcome) = self
                .existing_invoice_nwc_payment_outcome(invoice_id)
                .await?
            {
                return Ok(existing_outcome);
            }
            return Err(anyhow!(
                "invoice_nwc_payment row missing after insert race for invoice {invoice_id}"
            ));
        }

        match tenant_wallet.pay_invoice(bolt11).await {
            Ok(()) => match self.command.mark_invoice_nwc_payment_paid(invoice_id).await {
                Ok(()) => Ok(NwcInvoicePaymentOutcome::Paid),
                Err(error) => Ok(NwcInvoicePaymentOutcome::Pending(anyhow!(
                    "invoice {invoice_id} was charged over NWC but failed to persist paid state: {error}"
                ))),
            },
            Err(error) => Ok(NwcInvoicePaymentOutcome::Pending(anyhow!(
                "invoice {invoice_id} NWC payment attempt requires reconciliation: {error}"
            ))),
        }
    }
}

fn summarize_nwc_error_for_dm(error: &str) -> Option<String> {
    let normalized = error.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return None;
    }

    if normalized.chars().count() <= NWC_ERROR_DM_MAX_CHARS {
        return Some(normalized);
    }

    let prefix_len = NWC_ERROR_DM_MAX_CHARS.saturating_sub(3);
    let mut truncated = normalized.chars().take(prefix_len).collect::<String>();
    truncated.push_str("...");
    Some(truncated)
}

fn manual_lightning_payment_dm(nwc_error: Option<&str>) -> String {
    match nwc_error {
        Some(error) if !error.is_empty() => {
            format!("{MANUAL_LIGHTNING_PAYMENT_DM}\n\n{NWC_ERROR_DM_PREFIX} {error}")
        }
        _ => MANUAL_LIGHTNING_PAYMENT_DM.to_string(),
    }
}
