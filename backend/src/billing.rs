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

/// How long a freshly minted bolt11 stays valid. Once it lapses, an unpaid
/// invoice's bolt11 is regenerated on next access, so the tenant is never shown
/// a dead invoice and the sat amount stays pegged to the current BTC price.
const BOLT11_EXPIRY_SECS: i64 = 3600;

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

    pub async fn create_bolt11(&self, amount_due_minor: i64, currency: &str) -> Result<String> {
        let amount_msats = bitcoin::fiat_to_msats(amount_due_minor, currency).await?;
        self.wallet
            .make_invoice(
                amount_msats,
                LIGHTNING_INVOICE_DESCRIPTION,
                BOLT11_EXPIRY_SECS as u64,
            )
            .await
    }

    /// Return the current valid bolt11 for an open invoice, minting one if none
    /// exists and regenerating it if the stored one has expired. There is
    /// exactly one bolt11 per invoice: whoever pays it — the tenant's NWC wallet
    /// or a human — settles the same Lightning invoice, so the bolt11 itself is
    /// the double-charge guard.
    pub async fn ensure_lightning_invoice(
        &self,
        stripe_invoice_id: &str,
        tenant_pubkey: &str,
        amount_due_minor: i64,
        currency: &str,
    ) -> Result<String> {
        let now = chrono::Utc::now().timestamp();

        if let Some(existing) = self.query.get_lightning_invoice(stripe_invoice_id).await? {
            // Keep a still-valid invoice, or any bolt11 we've already settled.
            if existing.status != "pending" || now < existing.expires_at {
                return Ok(existing.bolt11);
            }
            // The stored invoice expired unpaid, so mint a fresh one. The old
            // invoice can no longer be paid, so no settlement can be missed.
            let bolt11 = self.create_bolt11(amount_due_minor, currency).await?;
            self.command
                .regenerate_lightning_invoice(stripe_invoice_id, &bolt11, now + BOLT11_EXPIRY_SECS)
                .await?;
            return Ok(bolt11);
        }

        let bolt11 = self.create_bolt11(amount_due_minor, currency).await?;
        if self
            .command
            .insert_pending_lightning_invoice(
                stripe_invoice_id,
                tenant_pubkey,
                &bolt11,
                now + BOLT11_EXPIRY_SECS,
            )
            .await?
        {
            Ok(bolt11)
        } else {
            // Lost the insert race; use whatever the winner stored.
            Ok(self
                .query
                .get_lightning_invoice(stripe_invoice_id)
                .await?
                .ok_or_else(|| {
                    anyhow!("lightning_invoice row missing after insert race for invoice {stripe_invoice_id}")
                })?
                .bolt11)
        }
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
                    stripe_invoice_id = %invoice.id,
                    "failed to retry card payment for outstanding invoice"
                );
            }
        }

        Ok(())
    }

    // --- Lightning / NWC orchestration ---

    /// Push a payment for an invoice's persisted bolt11 from the tenant's NWC
    /// wallet, then confirm settlement against the system wallet. On success the
    /// Stripe invoice is marked paid out of band and `Ok(())` is returned; on
    /// failure the returned error is the reason to surface to the tenant (the
    /// caller falls through to other payment methods rather than propagating
    /// it). Reusing the same bolt11 means a retry — or a concurrent manual
    /// payment — can never double-charge.
    pub async fn nwc_pay_invoice(
        &self,
        stripe_invoice_id: &str,
        tenant_pubkey: &str,
        bolt11: &str,
        tenant_nwc_url: &str,
    ) -> Result<()> {
        let tenant_wallet = Wallet::from_url(tenant_nwc_url)?;

        match tenant_wallet.pay_invoice(bolt11.to_string()).await {
            Ok(()) => self.settle_invoice(stripe_invoice_id, tenant_pubkey, "nwc").await,
            Err(pay_error) => {
                // The pay request errored, but the payment may have landed
                // before the response was lost. Confirm against the system
                // wallet before reporting failure.
                if self.wallet.is_settled(bolt11).await.unwrap_or(false) {
                    self.settle_invoice(stripe_invoice_id, tenant_pubkey, "nwc").await
                } else {
                    Err(pay_error)
                }
            }
        }
    }

    /// If an open invoice's bolt11 has settled on Lightning, record it as paid.
    /// This is the shared settlement path for both NWC payments that landed late
    /// and manual payments; it is driven by the actual Lightning settlement
    /// rather than our local state, so it self-corrects if a previous attempt
    /// updated Stripe but not our row (or vice versa).
    pub async fn reconcile_invoice(&self, invoice: &StripeInvoice) -> Result<StripeInvoice> {
        if invoice.status != "open" {
            return Ok(invoice.clone());
        }

        let Some(row) = self.query.get_lightning_invoice(&invoice.id).await? else {
            return Ok(invoice.clone());
        };

        let settled = match self.wallet.is_settled(&row.bolt11).await {
            Ok(settled) => settled,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    stripe_invoice_id = %invoice.id,
                    "failed to look up bolt11 invoice settlement"
                );
                return Ok(invoice.clone());
            }
        };

        if !settled {
            return Ok(invoice.clone());
        }

        if let Err(error) = self.settle_invoice(&invoice.id, &row.tenant_pubkey, "manual").await {
            tracing::warn!(
                error = %error,
                stripe_invoice_id = %invoice.id,
                "failed to mark settled bolt11 invoice as paid"
            );
        }

        // The invoice existed a moment ago; if Stripe suddenly returns 404, fall
        // back to the pre-reconcile snapshot rather than failing the request.
        Ok(self
            .stripe
            .get_invoice(&invoice.id)
            .await?
            .unwrap_or_else(|| invoice.clone()))
    }

    /// Record a settled bolt11 invoice as paid by `method`, mark the Stripe
    /// invoice paid out of band, and clear any lingering NWC error. The Stripe
    /// call is idempotent across retries, and `mark_lightning_invoice_paid` is
    /// first-writer-wins, so the recorded method reflects whoever settled first.
    async fn settle_invoice(
        &self,
        stripe_invoice_id: &str,
        tenant_pubkey: &str,
        method: &str,
    ) -> Result<()> {
        self.command
            .mark_lightning_invoice_paid(stripe_invoice_id, method)
            .await?;
        self.stripe.pay_invoice_out_of_band(stripe_invoice_id).await?;
        self.command.clear_tenant_nwc_error(tenant_pubkey).await?;
        Ok(())
    }
}
