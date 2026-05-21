use anyhow::{Result, anyhow};
use std::collections::BTreeMap;

use crate::bitcoin;
use crate::command::Command;
use crate::env::Env;
use crate::models::{Activity, Tenant, RELAY_STATUS_ACTIVE};
use crate::query::Query;
use crate::stripe::{Stripe, StripeInvoice, StripeSubscription};
use crate::wallet::Wallet;

const LIGHTNING_INVOICE_DESCRIPTION: &str = "Relay subscription payment";

pub enum NwcInvoicePaymentOutcome {
    Paid,
    Fallback(anyhow::Error),
    Pending(anyhow::Error),
}

#[derive(Clone)]
pub struct Billing {
    stripe: Stripe,
    wallet: Wallet,
    query: Query,
    command: Command,
}

impl Billing {
    pub fn new(query: Query, command: Command, env: &Env) -> Self {
        Self {
            stripe: Stripe::new(env),
            wallet: Wallet::from_url(&env.robot_wallet).expect("invalid ROBOT_WALLET"),
            query,
            command,
        }
    }

    pub async fn start(self) {
        let mut rx = self.command.notify.subscribe();

        if let Err(error) = self.reconcile_subscriptions("startup").await {
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

                    if let Err(error) = self.reconcile_subscriptions("lagged").await {
                        tracing::error!(error = %error, "failed to reconcile relay billing state after lag");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    async fn reconcile_subscriptions(&self, source: &str) -> Result<()> {
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
            if let Err(error) = self.sync_tenant(&tenant.pubkey).await {
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

        if needs_billing_sync {
            self.sync_tenant(&activity.tenant).await?;
        }

        Ok(())
    }

    /// Reconciles a tenant's single Stripe subscription with the set of relays that
    /// should be billed.
    ///
    /// Stripe forbids two subscription items on the same subscription from sharing a
    /// price, so billing is modeled as one subscription item per plan (price) with
    /// `quantity` equal to the number of the tenant's `active` relays on that plan.
    async fn sync_tenant(&self, tenant_pubkey: &str) -> Result<()> {
        let Some(tenant) = self.query.get_tenant(tenant_pubkey).await? else {
            return Ok(());
        };

        let quantity_by_price_id = self.get_quantity_by_price_id(&tenant).await?;

        // If we've got no subscription items, we can cancel and clear the tenant's subscription
        if quantity_by_price_id.is_empty() {
            self.ensure_subscription_is_inactive(&tenant).await?;
            return Ok(());
        }

        let subscription = self
            .ensure_subscription_is_active(&tenant, &quantity_by_price_id)
            .await?;

        self.ensure_subscription_items(subscription, quantity_by_price_id).await
    }

    // --Stripe helpers--

    /// Gets a map of stripe_price_id -> quantity based on the tenant's current relays
    async fn get_quantity_by_price_id(&self, tenant: &Tenant) -> Result<BTreeMap<String, i64>> {
        let mut quantity_by_price_id = BTreeMap::new();
        for relay in self.query.list_relays_for_tenant(&tenant.pubkey).await? {
            if relay.status != RELAY_STATUS_ACTIVE {
                continue;
            }
            let Some(price_id) = self.query.get_plan(&relay.plan).and_then(|p| p.stripe_price_id) else {
                continue;
            };
            *quantity_by_price_id.entry(price_id).or_insert(0) += 1;
        }
        Ok(quantity_by_price_id)
    }

    /// Fetch the tenant's current subscription from Stripe, if it has one
    async fn get_subscription(&self, tenant: &Tenant) -> Result<Option<StripeSubscription>> {
        let subscription = match &tenant.stripe_subscription_id {
            Some(id) => self.stripe.get_subscription(id).await?,
            None => None,
        };

        // If it's canceled, clear the subscription id and return nothing for simplicity
        if subscription
            .as_ref()
            .is_some_and(|s| matches!(s.status.as_str(), "canceled" | "incomplete_expired"))
        {
            self.command.clear_tenant_subscription(&tenant.pubkey).await?;
            return Ok(None);
        }

        Ok(subscription)
    }

    /// Make sure the tenant has an active subscription, creating one with the desired
    /// items if it doesn't (Stripe rejects an itemless subscription).
    async fn ensure_subscription_is_active(
        &self,
        tenant: &Tenant,
        quantity_by_price_id: &BTreeMap<String, i64>,
    ) -> Result<StripeSubscription> {
        if let Some(sub) = self.get_subscription(tenant).await? {
            return Ok(sub);
        }

        let sub = self
            .stripe
            .create_subscription(&tenant.stripe_customer_id, quantity_by_price_id)
            .await?;
        self.command.set_tenant_subscription(&tenant.pubkey, &sub.id).await?;
        Ok(sub)
    }

    /// If the tenant has a subscription, cancel and clear it
    async fn ensure_subscription_is_inactive(&self, tenant: &Tenant) -> Result<()> {
        if let Some(s) = self.get_subscription(tenant).await? {
            self.stripe.cancel_subscription(&s.id).await?;
            self.command.clear_tenant_subscription(&tenant.pubkey).await?;
        }

        Ok(())
    }

    /// Sync desired quantity_by_price_id with stripe
    async fn ensure_subscription_items(
        &self,
        subscription: StripeSubscription,
        quantity_by_price_id: BTreeMap<String, i64>,
    ) -> Result<()> {
        let mut current: BTreeMap<String, (String, i64)> = BTreeMap::new();
        for item in subscription.items {
            current.insert(item.price.id, (item.id, item.quantity));
        }

        for (price_id, &quantity) in &quantity_by_price_id {
            if let Some((item_id, current_quantity)) = current.remove(price_id) {
                if current_quantity != quantity {
                    self.stripe
                        .set_subscription_item_quantity(&item_id, quantity)
                        .await?;
                }
            } else {
                self.stripe
                    .create_subscription_item(&subscription.id, price_id, quantity)
                    .await?;
            }
        }

        for (_, (item_id, _)) in current {
            self.stripe.delete_subscription_item(&item_id).await?;
        }

        Ok(())
    }

    // --- Public API helpers ---

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

    pub async fn pay_outstanding_card_invoices(
        &self,
        tenant: &crate::models::Tenant,
    ) -> Result<()> {
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

    pub async fn mark_invoice_paid_out_of_band_after_nwc(
        &self,
        invoice_id: &str,
        tenant_pubkey: &str,
    ) -> Result<()> {
        self.stripe.pay_invoice_out_of_band(invoice_id).await?;
        self.command.clear_tenant_nwc_error(tenant_pubkey).await?;
        Ok(())
    }

    pub async fn reconcile_manual_lightning_invoice(
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
    pub async fn nwc_pay_invoice(
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
}
