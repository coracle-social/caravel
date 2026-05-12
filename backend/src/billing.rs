use anyhow::{Result, anyhow};
use hmac::{Hmac, Mac};
use nwc::prelude::{
    LookupInvoiceRequest, LookupInvoiceResponse, MakeInvoiceRequest, NWC, NostrWalletConnectURI,
    PayInvoiceRequest as NwcPayInvoiceRequest, TransactionState,
};
use sha2::Sha256;
use std::collections::BTreeMap;

use crate::command::Command;
use crate::models::{Activity, RELAY_STATUS_ACTIVE, RELAY_STATUS_DELINQUENT, Relay};
use crate::query::Query;
use crate::robot::Robot;

type HmacSha256 = Hmac<Sha256>;

const STRIPE_API: &str = "https://api.stripe.com/v1";
const COINBASE_SPOT_API: &str = "https://api.coinbase.com/v2/prices";
const WEBHOOK_TOLERANCE_SECS: i64 = 300;
const MANUAL_LIGHTNING_PAYMENT_DM: &str = "Payment is due for your relay subscription. Please visit the application to complete a manual Lightning payment.";
const NWC_ERROR_DM_PREFIX: &str = "NWC auto-payment failed:";
const NWC_ERROR_DM_MAX_CHARS: usize = 240;

#[derive(Debug)]
pub enum InvoiceLookupError {
    StripeClient { status: reqwest::StatusCode },
    Internal(anyhow::Error),
}

impl std::fmt::Display for InvoiceLookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StripeClient { status } => {
                write!(
                    f,
                    "stripe invoice lookup failed with status {}",
                    status.as_u16()
                )
            }
            Self::Internal(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for InvoiceLookupError {}

impl From<anyhow::Error> for InvoiceLookupError {
    fn from(value: anyhow::Error) -> Self {
        Self::Internal(value)
    }
}

impl From<reqwest::Error> for InvoiceLookupError {
    fn from(value: reqwest::Error) -> Self {
        Self::Internal(value.into())
    }
}

#[derive(serde::Deserialize)]
struct StripeEvent {
    #[serde(rename = "type")]
    event_type: String,
    data: StripeEventData,
}

#[derive(serde::Deserialize)]
struct StripeEventData {
    object: serde_json::Value,
}

#[derive(serde::Deserialize)]
struct CoinbaseSpotPriceResponse {
    data: CoinbaseSpotPriceData,
}

#[derive(serde::Deserialize)]
struct CoinbaseSpotPriceData {
    amount: String,
}

enum NwcInvoicePaymentOutcome {
    Paid,
    Fallback(anyhow::Error),
    Pending(anyhow::Error),
}

#[derive(Clone)]
pub struct Billing {
    nwc_url: String,
    stripe_secret_key: String,
    stripe_webhook_secret: String,
    http: reqwest::Client,
    query: Query,
    command: Command,
    robot: Robot,
}

impl Billing {
    pub fn new(query: Query, command: Command, robot: Robot) -> Self {
        let nwc_url = std::env::var("NWC_URL").unwrap_or_default();
        let stripe_secret_key = std::env::var("STRIPE_SECRET_KEY").unwrap_or_default();
        if stripe_secret_key.trim().is_empty() {
            panic!("missing STRIPE_SECRET_KEY environment variable");
        }
        let stripe_webhook_secret = std::env::var("STRIPE_WEBHOOK_SECRET").unwrap_or_default();
        if stripe_webhook_secret.trim().is_empty() {
            panic!("missing STRIPE_WEBHOOK_SECRET environment variable");
        }
        Self {
            nwc_url,
            stripe_secret_key,
            stripe_webhook_secret,
            http: reqwest::Client::new(),
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

        if needs_billing_sync {
            self.sync_relay_subscription(activity).await?;
        }

        Ok(())
    }

    pub async fn sync_relay_subscription(&self, activity: &Activity) -> Result<()> {
        let Some(relay) = self.query.get_relay(&activity.resource_id).await? else {
            return Ok(());
        };

        self.sync_tenant_subscription(&relay.tenant).await
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
            let Some(plan) = Query::get_plan(&relay.plan) else {
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
            Some(subscription_id) => match self.stripe_get_subscription(subscription_id).await? {
                Some(sub)
                    if !matches!(
                        sub["status"].as_str().unwrap_or_default(),
                        "canceled" | "incomplete_expired"
                    ) =>
                {
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
                self.stripe_cancel_subscription(subscription_id).await?;
                self.command
                    .clear_tenant_subscription(tenant_pubkey)
                    .await?;
            }
            for relay in &relays {
                if relay.stripe_subscription_item_id.is_some() {
                    self.command
                        .delete_relay_subscription_item(&relay.id)
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
                let (subscription_id, items) = self
                    .stripe_create_subscription(&tenant.stripe_customer_id, &desired)
                    .await?;
                self.command
                    .set_tenant_subscription(tenant_pubkey, &subscription_id)
                    .await?;
                tenant.stripe_subscription_id = Some(subscription_id);
                price_to_item = items;
            }
            Some(sub) => {
                let subscription_id = sub["id"]
                    .as_str()
                    .ok_or_else(|| anyhow!("missing subscription id"))?
                    .to_string();

                // price id -> (item id, quantity) for items currently on the subscription.
                let mut current: BTreeMap<String, (String, i64)> = BTreeMap::new();
                for item in sub["items"]["data"].as_array().into_iter().flatten() {
                    let (Some(item_id), Some(price_id)) =
                        (item["id"].as_str(), item["price"]["id"].as_str())
                    else {
                        continue;
                    };
                    let quantity = item["quantity"].as_i64().unwrap_or(1);
                    current.insert(price_id.to_string(), (item_id.to_string(), quantity));
                }

                for (price_id, &quantity) in &desired {
                    if let Some((item_id, current_quantity)) = current.remove(price_id) {
                        if current_quantity != quantity {
                            if quantity < current_quantity {
                                downgraded = true;
                            }
                            self.stripe_set_subscription_item_quantity(&item_id, quantity)
                                .await?;
                        }
                        price_to_item.insert(price_id.clone(), item_id);
                    } else {
                        let item_id = self
                            .stripe_create_subscription_item(&subscription_id, price_id, quantity)
                            .await?;
                        price_to_item.insert(price_id.clone(), item_id);
                    }
                }

                // Items for plans no relay is on anymore.
                for (_, (item_id, _)) in current {
                    downgraded = true;
                    self.stripe_delete_subscription_item(&item_id).await?;
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
                            .delete_relay_subscription_item(&relay.id)
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
        self.verify_webhook_signature(payload, signature)?;

        let event: StripeEvent = serde_json::from_str(payload)?;
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

    fn verify_webhook_signature(&self, payload: &str, sig_header: &str) -> Result<()> {
        let mut timestamp = None;
        let mut signature = None;
        for part in sig_header.split(',') {
            if let Some(t) = part.strip_prefix("t=") {
                timestamp = Some(t);
            } else if let Some(v) = part.strip_prefix("v1=") {
                signature = Some(v);
            }
        }
        let timestamp = timestamp.ok_or_else(|| anyhow!("missing webhook timestamp"))?;
        let signature = signature.ok_or_else(|| anyhow!("missing webhook signature"))?;

        let signed_payload = format!("{timestamp}.{payload}");
        let mut mac = HmacSha256::new_from_slice(self.stripe_webhook_secret.as_bytes())
            .map_err(|e| anyhow!("invalid webhook secret: {e}"))?;
        mac.update(signed_payload.as_bytes());
        let expected = hex::encode(mac.finalize().into_bytes());

        if expected != signature {
            return Err(anyhow!("webhook signature mismatch"));
        }

        let ts: i64 = timestamp
            .parse()
            .map_err(|_| anyhow!("bad webhook timestamp"))?;
        let now = chrono::Utc::now().timestamp();
        if (now - ts).abs() > WEBHOOK_TOLERANCE_SECS {
            return Err(anyhow!("webhook timestamp outside tolerance"));
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
            let plain_nwc_url = crate::cipher::decrypt(&tenant.nwc_url)?;
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
            .stripe_has_payment_method(&tenant.stripe_customer_id)
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
                if Self::should_reactivate_after_payment(&relay) {
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
            if relay.status == RELAY_STATUS_ACTIVE && Query::is_paid_plan(&relay.plan) {
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
            if relay.status == RELAY_STATUS_ACTIVE && Query::is_paid_plan(&relay.plan) {
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
            .stripe_preview_upcoming_invoice(
                &tenant.stripe_customer_id,
                tenant.stripe_subscription_id.as_deref(),
            )
            .await
        {
            Ok(upcoming) => {
                let lines = upcoming["lines"]["data"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let proration_lines = lines
                    .iter()
                    .filter(|line| line["proration"].as_bool().unwrap_or(false))
                    .count();
                let amount_due = upcoming["amount_due"]
                    .as_i64()
                    .unwrap_or_else(|| upcoming["total"].as_i64().unwrap_or(0));
                let currency = upcoming["currency"].as_str().unwrap_or("usd");
                let preview_id = upcoming["id"].as_str().unwrap_or_default();

                tracing::info!(
                    tenant_pubkey = %tenant.pubkey,
                    stripe_customer_id = %tenant.stripe_customer_id,
                    context,
                    preview_id,
                    proration_lines,
                    amount_due,
                    currency,
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

    pub async fn get_invoice_with_tenant(
        &self,
        invoice_id: &str,
    ) -> std::result::Result<(serde_json::Value, crate::models::Tenant), InvoiceLookupError> {
        let invoice = self.stripe_get_invoice(invoice_id).await?;
        let customer_id = invoice["customer"]
            .as_str()
            .ok_or_else(|| InvoiceLookupError::Internal(anyhow!("invoice missing customer")))?;
        let tenant = self
            .query
            .get_tenant_by_stripe_customer_id(customer_id)
            .await?
            .ok_or_else(|| {
                InvoiceLookupError::Internal(anyhow!("tenant not found for customer"))
            })?;
        Ok((invoice, tenant))
    }

    pub async fn reconcile_manual_lightning_invoice(
        &self,
        invoice_id: &str,
        invoice: &serde_json::Value,
    ) -> std::result::Result<serde_json::Value, InvoiceLookupError> {
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
        let nostr_name = self.robot.fetch_nostr_name(tenant_pubkey).await;
        let display_name = nostr_name.unwrap_or_else(|| short_pubkey.clone());
        let idempotency_key = self.idempotency_key(&["create_customer", tenant_pubkey]);

        let resp = self
            .http
            .post(format!("{STRIPE_API}/customers"))
            .bearer_auth(&self.stripe_secret_key)
            .header("Idempotency-Key", idempotency_key)
            .form(&[
                ("name", display_name.as_str()),
                ("metadata[tenant_pubkey]", tenant_pubkey),
            ])
            .send()
            .await?;

        let body: serde_json::Value = stripe_error_for_status(resp).await?.json().await?;
        let customer_id = body["id"]
            .as_str()
            .ok_or_else(|| anyhow!("missing customer id"))?;

        if !customer_id.starts_with("cus_") {
            return Err(anyhow!("unexpected customer id format"));
        }

        Ok(customer_id.to_string())
    }

    pub async fn stripe_list_invoices(&self, customer_id: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .get(format!("{STRIPE_API}/invoices"))
            .bearer_auth(&self.stripe_secret_key)
            .query(&[("customer", customer_id)])
            .send()
            .await?;

        let body: serde_json::Value = stripe_error_for_status(resp).await?.json().await?;
        Ok(body["data"].clone())
    }

    pub async fn stripe_get_invoice(
        &self,
        invoice_id: &str,
    ) -> std::result::Result<serde_json::Value, InvoiceLookupError> {
        let resp = self
            .http
            .get(format!("{STRIPE_API}/invoices/{invoice_id}"))
            .bearer_auth(&self.stripe_secret_key)
            .send()
            .await?;

        if resp.status().is_client_error() {
            return Err(InvoiceLookupError::StripeClient {
                status: resp.status(),
            });
        }

        let body: serde_json::Value = stripe_error_for_status(resp).await?.json().await?;
        Ok(body)
    }

    pub async fn create_bolt11(&self, amount_due_minor: i64, currency: &str) -> Result<String> {
        let amount_msats = self.fiat_minor_to_msats(amount_due_minor, currency).await?;

        let system_uri: NostrWalletConnectURI = self
            .nwc_url
            .parse()
            .map_err(|_| anyhow!("invalid system NWC URL"))?;
        let system_nwc = NWC::new(system_uri);

        let make_req = MakeInvoiceRequest {
            amount: amount_msats,
            description: Some("Relay subscription payment".to_string()),
            description_hash: None,
            expiry: None,
        };

        let invoice_response = system_nwc
            .make_invoice(make_req)
            .await
            .map_err(|e| anyhow!("failed to create invoice: {e}"))?;

        system_nwc.shutdown().await;

        Ok(invoice_response.invoice)
    }

    pub async fn pay_outstanding_nwc_invoices(&self, tenant: &crate::models::Tenant) -> Result<()> {
        if tenant.nwc_url.is_empty() {
            return Ok(());
        }

        let plain_nwc_url = crate::cipher::decrypt(&tenant.nwc_url)?;

        let invoices = self
            .stripe_list_invoices(&tenant.stripe_customer_id)
            .await?;
        let invoices_arr = invoices.as_array().cloned().unwrap_or_default();

        for invoice in &invoices_arr {
            let status = invoice["status"].as_str().unwrap_or_default();
            let amount_due = invoice["amount_due"].as_i64().unwrap_or(0);
            let invoice_id = invoice["id"].as_str().unwrap_or_default();
            let currency = invoice["currency"].as_str().unwrap_or("usd");

            if status != "open" || amount_due == 0 || invoice_id.is_empty() {
                continue;
            }

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
            .stripe_has_payment_method(&tenant.stripe_customer_id)
            .await?
        {
            return Ok(());
        }

        let invoices = self
            .stripe_list_invoices(&tenant.stripe_customer_id)
            .await?;
        let invoices_arr = invoices.as_array().cloned().unwrap_or_default();

        for invoice in &invoices_arr {
            let status = invoice["status"].as_str().unwrap_or_default();
            let amount_due = invoice["amount_due"].as_i64().unwrap_or(0);
            let invoice_id = invoice["id"].as_str().unwrap_or_default();

            if status != "open" || amount_due == 0 || invoice_id.is_empty() {
                continue;
            }

            if let Err(error) = self.stripe_pay_invoice(invoice_id).await {
                tracing::error!(
                    error = %error,
                    invoice_id,
                    "failed to retry card payment for outstanding invoice"
                );
            }
        }

        Ok(())
    }

    pub async fn stripe_create_portal_session(
        &self,
        customer_id: &str,
        return_url: Option<&str>,
    ) -> Result<String> {
        let mut params = vec![("customer", customer_id.to_string())];
        if let Some(url) = return_url {
            params.push(("return_url", url.to_string()));
        }
        let resp = self
            .http
            .post(format!("{STRIPE_API}/billing_portal/sessions"))
            .bearer_auth(&self.stripe_secret_key)
            .form(&params)
            .send()
            .await?;

        let body: serde_json::Value = stripe_error_for_status(resp).await?.json().await?;
        let url = body["url"]
            .as_str()
            .ok_or_else(|| anyhow!("missing portal session url"))?
            .to_string();

        Ok(url)
    }

    // --- Stripe API helpers ---

    fn idempotency_key(&self, parts: &[&str]) -> String {
        let mut mac = HmacSha256::new_from_slice(self.stripe_secret_key.as_bytes())
            .expect("HMAC accepts any key length");
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                mac.update(b":");
            }
            mac.update(part.as_bytes());
        }
        hex::encode(mac.finalize().into_bytes())
    }

    /// Fetches a subscription, returning `None` if Stripe no longer knows about it
    /// (so callers can recover from a stale `stripe_subscription_id`).
    async fn stripe_get_subscription(
        &self,
        subscription_id: &str,
    ) -> Result<Option<serde_json::Value>> {
        let resp = self
            .http
            .get(format!("{STRIPE_API}/subscriptions/{subscription_id}"))
            .bearer_auth(&self.stripe_secret_key)
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let body: serde_json::Value = stripe_error_for_status(resp).await?.json().await?;
        Ok(Some(body))
    }

    /// Creates a subscription with one item per `(price_id, quantity)` entry. Returns
    /// the subscription id and a map from price id to the created subscription item id.
    async fn stripe_create_subscription(
        &self,
        customer_id: &str,
        items: &BTreeMap<String, i64>,
    ) -> Result<(String, BTreeMap<String, String>)> {
        let mut form: Vec<(String, String)> = vec![
            ("customer".to_string(), customer_id.to_string()),
            (
                "collection_method".to_string(),
                "charge_automatically".to_string(),
            ),
        ];
        let mut key_parts: Vec<String> =
            vec!["create_subscription".to_string(), customer_id.to_string()];
        for (index, (price_id, quantity)) in items.iter().enumerate() {
            form.push((format!("items[{index}][price]"), price_id.clone()));
            form.push((format!("items[{index}][quantity]"), quantity.to_string()));
            key_parts.push(format!("{price_id}={quantity}"));
        }
        let key_refs: Vec<&str> = key_parts.iter().map(String::as_str).collect();
        let idempotency_key = self.idempotency_key(&key_refs);

        let resp = self
            .http
            .post(format!("{STRIPE_API}/subscriptions"))
            .bearer_auth(&self.stripe_secret_key)
            .header("Idempotency-Key", idempotency_key)
            .form(&form)
            .send()
            .await?;

        let body: serde_json::Value = stripe_error_for_status(resp).await?.json().await?;
        let subscription_id = body["id"]
            .as_str()
            .ok_or_else(|| anyhow!("missing subscription id"))?
            .to_string();
        let mut price_to_item = BTreeMap::new();
        for item in body["items"]["data"]
            .as_array()
            .ok_or_else(|| anyhow!("missing subscription items"))?
        {
            let item_id = item["id"]
                .as_str()
                .ok_or_else(|| anyhow!("missing subscription item id"))?;
            let price_id = item["price"]["id"]
                .as_str()
                .ok_or_else(|| anyhow!("missing subscription item price id"))?;
            price_to_item.insert(price_id.to_string(), item_id.to_string());
        }

        Ok((subscription_id, price_to_item))
    }

    async fn stripe_create_subscription_item(
        &self,
        subscription_id: &str,
        price_id: &str,
        quantity: i64,
    ) -> Result<String> {
        let idempotency_key =
            self.idempotency_key(&["create_subscription_item", subscription_id, price_id]);
        let quantity = quantity.to_string();
        let resp = self
            .http
            .post(format!("{STRIPE_API}/subscription_items"))
            .bearer_auth(&self.stripe_secret_key)
            .header("Idempotency-Key", idempotency_key)
            .form(&[
                ("subscription", subscription_id),
                ("price", price_id),
                ("quantity", quantity.as_str()),
            ])
            .send()
            .await?;

        let body: serde_json::Value = stripe_error_for_status(resp).await?.json().await?;
        let item_id = body["id"]
            .as_str()
            .ok_or_else(|| anyhow!("missing subscription item id"))?
            .to_string();

        Ok(item_id)
    }

    /// Sets a subscription item's quantity. No idempotency key: this is a
    /// reconcile-to-desired-state write, and re-applying the same target is a no-op.
    async fn stripe_set_subscription_item_quantity(
        &self,
        item_id: &str,
        quantity: i64,
    ) -> Result<()> {
        let resp = self
            .http
            .post(format!("{STRIPE_API}/subscription_items/{item_id}"))
            .bearer_auth(&self.stripe_secret_key)
            .form(&[("quantity", quantity.to_string())])
            .send()
            .await?;
        stripe_error_for_status(resp).await?;

        Ok(())
    }

    async fn stripe_delete_subscription_item(&self, item_id: &str) -> Result<()> {
        let resp = self
            .http
            .delete(format!("{STRIPE_API}/subscription_items/{item_id}"))
            .bearer_auth(&self.stripe_secret_key)
            .send()
            .await?;
        stripe_error_for_status(resp).await?;

        Ok(())
    }

    async fn stripe_cancel_subscription(&self, subscription_id: &str) -> Result<()> {
        let resp = self
            .http
            .delete(format!("{STRIPE_API}/subscriptions/{subscription_id}"))
            .bearer_auth(&self.stripe_secret_key)
            .send()
            .await?;
        stripe_error_for_status(resp).await?;

        Ok(())
    }

    async fn stripe_pay_invoice(&self, invoice_id: &str) -> Result<()> {
        let idempotency_key = self.idempotency_key(&["pay_invoice", invoice_id]);
        let resp = self
            .http
            .post(format!("{STRIPE_API}/invoices/{invoice_id}/pay"))
            .bearer_auth(&self.stripe_secret_key)
            .header("Idempotency-Key", idempotency_key)
            .send()
            .await?;
        stripe_error_for_status(resp).await?;

        Ok(())
    }

    async fn stripe_preview_upcoming_invoice(
        &self,
        customer_id: &str,
        subscription_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut req = self
            .http
            .get(format!("{STRIPE_API}/invoices/upcoming"))
            .bearer_auth(&self.stripe_secret_key)
            .query(&[("customer", customer_id)]);

        if let Some(subscription_id) = subscription_id {
            req = req.query(&[("subscription", subscription_id)]);
        }

        let body: serde_json::Value = stripe_error_for_status(req.send().await?)
            .await?
            .json()
            .await?;
        Ok(body)
    }

    async fn stripe_pay_invoice_out_of_band(&self, invoice_id: &str) -> Result<()> {
        let idempotency_key = self.idempotency_key(&["pay_invoice_oob", invoice_id]);
        let resp = self
            .http
            .post(format!("{STRIPE_API}/invoices/{invoice_id}/pay"))
            .bearer_auth(&self.stripe_secret_key)
            .header("Idempotency-Key", idempotency_key)
            .form(&[("paid_out_of_band", "true")])
            .send()
            .await?;
        stripe_error_for_status(resp).await?;

        Ok(())
    }

    async fn stripe_has_payment_method(&self, customer_id: &str) -> Result<bool> {
        let resp = self
            .http
            .get(format!("{STRIPE_API}/payment_methods"))
            .bearer_auth(&self.stripe_secret_key)
            .query(&[("customer", customer_id), ("type", "card")])
            .send()
            .await?;

        let body: serde_json::Value = stripe_error_for_status(resp).await?.json().await?;
        let has_method = body["data"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false);

        Ok(has_method)
    }

    // --- NWC helpers ---

    async fn mark_invoice_paid_out_of_band_after_nwc(
        &self,
        invoice_id: &str,
        tenant_pubkey: &str,
    ) -> Result<()> {
        self.stripe_pay_invoice_out_of_band(invoice_id).await?;
        self.command.clear_tenant_nwc_error(tenant_pubkey).await?;
        Ok(())
    }

    async fn reconcile_manual_lightning_invoice_if_settled(
        &self,
        invoice_id: &str,
        invoice: &serde_json::Value,
    ) -> std::result::Result<serde_json::Value, InvoiceLookupError> {
        if invoice["status"].as_str().unwrap_or_default() != "open" {
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

        if let Err(error) = self.stripe_pay_invoice_out_of_band(invoice_id).await {
            tracing::warn!(
                error = %error,
                invoice_id,
                "failed to mark settled manual lightning invoice as paid_out_of_band"
            );
        }

        self.stripe_get_invoice(invoice_id).await
    }

    async fn is_manual_lightning_invoice_settled(&self, bolt11: &str) -> Result<bool> {
        let system_uri = Self::parse_nwc_uri(&self.nwc_url, "system")?;
        let system_nwc = NWC::new(system_uri);

        let lookup_req = LookupInvoiceRequest {
            payment_hash: None,
            invoice: Some(bolt11.to_string()),
        };

        let lookup_result = system_nwc.lookup_invoice(lookup_req).await;
        system_nwc.shutdown().await;

        let lookup_response =
            lookup_result.map_err(|error| anyhow!("failed to lookup invoice: {error}"))?;

        Ok(Self::lookup_invoice_response_is_settled(&lookup_response))
    }

    fn lookup_invoice_response_is_settled(response: &LookupInvoiceResponse) -> bool {
        response.state == Some(TransactionState::Settled) || response.settled_at.is_some()
    }

    fn parse_nwc_uri(nwc_url: &str, role: &str) -> Result<NostrWalletConnectURI> {
        nwc_url
            .parse::<NostrWalletConnectURI>()
            .map_err(|_| anyhow!("invalid {role} NWC URL"))
    }

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

        let amount_msats = match self.fiat_minor_to_msats(amount_due_minor, currency).await {
            Ok(amount_msats) => amount_msats,
            Err(error) => return Ok(NwcInvoicePaymentOutcome::Fallback(error)),
        };

        // Create a bolt11 invoice using the system wallet (self.nwc_url)
        let system_uri = match Self::parse_nwc_uri(&self.nwc_url, "system") {
            Ok(system_uri) => system_uri,
            Err(error) => return Ok(NwcInvoicePaymentOutcome::Fallback(error)),
        };
        let system_nwc = NWC::new(system_uri);

        let make_req = MakeInvoiceRequest {
            amount: amount_msats,
            description: Some("Relay subscription payment".to_string()),
            description_hash: None,
            expiry: None,
        };

        let invoice_response = system_nwc.make_invoice(make_req).await;

        let invoice_response = match invoice_response {
            Ok(invoice_response) => invoice_response,
            Err(error) => {
                system_nwc.shutdown().await;
                return Ok(NwcInvoicePaymentOutcome::Fallback(anyhow!(
                    "failed to create invoice: {error}"
                )));
            }
        };

        system_nwc.shutdown().await;

        // Pay the bolt11 invoice using the tenant's wallet
        let tenant_uri = match Self::parse_nwc_uri(tenant_nwc_url, "tenant") {
            Ok(tenant_uri) => tenant_uri,
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

        let tenant_nwc = NWC::new(tenant_uri);

        let pay_req = NwcPayInvoiceRequest::new(invoice_response.invoice);

        let pay_result = tenant_nwc.pay_invoice(pay_req).await;

        tenant_nwc.shutdown().await;

        match pay_result {
            Ok(_) => match self.command.mark_invoice_nwc_payment_paid(invoice_id).await {
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

    async fn fiat_minor_to_msats(&self, amount_due_minor: i64, currency: &str) -> Result<u64> {
        let normalized_currency = currency.to_uppercase();
        let btc_price = self.fetch_btc_spot_price(&normalized_currency).await?;
        fiat_minor_to_msats_from_quote(amount_due_minor, &normalized_currency, btc_price)
    }

    fn should_reactivate_after_payment(relay: &Relay) -> bool {
        relay.status == RELAY_STATUS_DELINQUENT && Query::is_paid_plan(&relay.plan)
    }

    async fn fetch_btc_spot_price(&self, currency: &str) -> Result<f64> {
        fetch_btc_spot_price_from_base(&self.http, COINBASE_SPOT_API, currency).await
    }

    fn currency_minor_exponent(currency: &str) -> Result<u8> {
        let normalized = currency.to_uppercase();
        let exponent = match normalized.as_str() {
            // Zero-decimal currencies in Stripe.
            "BIF" | "CLP" | "DJF" | "GNF" | "JPY" | "KMF" | "KRW" | "MGA" | "PYG" | "RWF"
            | "UGX" | "VND" | "VUV" | "XAF" | "XOF" | "XPF" => 0,
            // Three-decimal currencies in Stripe.
            "BHD" | "JOD" | "KWD" | "OMR" | "TND" => 3,
            _ if normalized.chars().all(|c| c.is_ascii_alphabetic()) && normalized.len() == 3 => 2,
            _ => return Err(anyhow!("invalid currency code: {currency}")),
        };

        Ok(exponent)
    }
}

/// Like [`reqwest::Response::error_for_status`], but on a 4xx/5xx response it reads
/// the body and folds Stripe's JSON error payload (`error.message`/`code`/`param`)
/// into the returned error, so callers get an actionable message instead of a bare
/// "400 Bad Request" with only the URL.
async fn stripe_error_for_status(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if !status.is_client_error() && !status.is_server_error() {
        return Ok(resp);
    }

    let url = resp.url().clone();
    let body = resp.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|json| {
            let error = &json["error"];
            let message = error["message"].as_str()?.to_string();
            let mut detail = message;
            if let Some(code) = error["type"].as_str().or_else(|| error["code"].as_str()) {
                detail.push_str(&format!(" [{code}]"));
            }
            if let Some(param) = error["param"].as_str() {
                detail.push_str(&format!(" (param: {param})"));
            }
            Some(detail)
        })
        .unwrap_or_else(|| {
            if body.trim().is_empty() {
                "<empty response body>".to_string()
            } else {
                body
            }
        });

    Err(anyhow!(
        "Stripe API request to {url} failed with status {status}: {detail}"
    ))
}

pub async fn fetch_btc_spot_price_from_base(
    http: &reqwest::Client,
    api_base: &str,
    currency: &str,
) -> Result<f64> {
    let pair = format!("BTC-{currency}");
    let url = format!("{}/{pair}/spot", api_base.trim_end_matches('/'));
    let resp = http.get(url).send().await?;
    let body: CoinbaseSpotPriceResponse = resp.error_for_status()?.json().await?;

    let amount = body
        .data
        .amount
        .parse::<f64>()
        .map_err(|e| anyhow!("invalid BTC spot quote for {currency}: {e}"))?;

    if amount <= 0.0 {
        return Err(anyhow!(
            "invalid non-positive BTC spot quote for {currency}"
        ));
    }

    Ok(amount)
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

pub fn fiat_minor_to_msats_from_quote(
    amount_due_minor: i64,
    currency: &str,
    btc_price_in_fiat: f64,
) -> Result<u64> {
    if amount_due_minor <= 0 {
        return Err(anyhow!("amount_due must be positive"));
    }

    if btc_price_in_fiat <= 0.0 {
        return Err(anyhow!("btc_price_in_fiat must be positive"));
    }

    let exponent = Billing::currency_minor_exponent(currency)?;
    let divisor = 10_f64.powi(exponent as i32);
    let amount_fiat = (amount_due_minor as f64) / divisor;
    let amount_btc = amount_fiat / btc_price_in_fiat;
    let raw_msats = amount_btc * 100_000_000_000.0;
    // Guard against tiny floating point artifacts at integer boundaries.
    let amount_msats = if (raw_msats - raw_msats.round()).abs() < 1e-6 {
        raw_msats.round()
    } else {
        raw_msats.ceil()
    };

    if !amount_msats.is_finite() || amount_msats <= 0.0 || amount_msats > u64::MAX as f64 {
        return Err(anyhow!("calculated msat amount is out of bounds"));
    }

    Ok(amount_msats as u64)
}

#[cfg(test)]
mod tests {
    use super::{Billing, fiat_minor_to_msats_from_quote};
    use crate::models::{
        RELAY_STATUS_ACTIVE, RELAY_STATUS_DELINQUENT, RELAY_STATUS_INACTIVE, Relay,
    };

    fn relay_fixture(status: &str, plan: &str) -> Relay {
        Relay {
            id: "relay-1".to_string(),
            tenant: "tenant-1".to_string(),
            schema: "tenant_1".to_string(),
            subdomain: "relay-1".to_string(),
            plan: plan.to_string(),
            stripe_subscription_item_id: None,
            status: status.to_string(),
            sync_error: String::new(),
            info_name: String::new(),
            info_icon: String::new(),
            info_description: String::new(),
            policy_public_join: 0,
            policy_strip_signatures: 0,
            groups_enabled: 1,
            management_enabled: 1,
            blossom_enabled: 1,
            livekit_enabled: 1,
            push_enabled: 1,
            synced: 1,
        }
    }

    #[test]
    fn converts_usd_minor_units_with_quote() {
        let msats = fiat_minor_to_msats_from_quote(100, "usd", 100_000.0)
            .expect("conversion should succeed");
        assert_eq!(msats, 1_000_000);
    }

    #[test]
    fn converts_zero_decimal_currency_with_quote() {
        let msats = fiat_minor_to_msats_from_quote(100, "jpy", 10_000_000.0)
            .expect("conversion should succeed");
        assert_eq!(msats, 1_000_000);
    }

    #[test]
    fn reactivates_only_delinquent_paid_relays_after_payment() {
        let delinquent_paid = relay_fixture(RELAY_STATUS_DELINQUENT, "basic");
        assert!(Billing::should_reactivate_after_payment(&delinquent_paid));

        let manually_inactive_paid = relay_fixture(RELAY_STATUS_INACTIVE, "basic");
        assert!(!Billing::should_reactivate_after_payment(
            &manually_inactive_paid
        ));

        let free_delinquent = relay_fixture(RELAY_STATUS_DELINQUENT, "free");
        assert!(!Billing::should_reactivate_after_payment(&free_delinquent));

        let active_paid = relay_fixture(RELAY_STATUS_ACTIVE, "basic");
        assert!(!Billing::should_reactivate_after_payment(&active_paid));

        let unknown_status_paid = relay_fixture("suspended", "basic");
        assert!(!Billing::should_reactivate_after_payment(
            &unknown_status_paid
        ));
    }

    use super::*;
    use sqlx::SqlitePool;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    use std::sync::OnceLock;
    use tokio::sync::Mutex;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[allow(unused_unsafe)]
    fn set_stripe_secret_key(value: Option<&str>) {
        match value {
            Some(v) => unsafe { std::env::set_var("STRIPE_SECRET_KEY", v) },
            None => unsafe { std::env::remove_var("STRIPE_SECRET_KEY") },
        }
    }

    #[allow(unused_unsafe)]
    fn set_stripe_webhook_secret(value: Option<&str>) {
        match value {
            Some(v) => unsafe { std::env::set_var("STRIPE_WEBHOOK_SECRET", v) },
            None => unsafe { std::env::remove_var("STRIPE_WEBHOOK_SECRET") },
        }
    }

    struct StripeSecretKeyGuard {
        previous: Option<String>,
    }

    impl StripeSecretKeyGuard {
        fn set(value: Option<&str>) -> Self {
            let previous = std::env::var("STRIPE_SECRET_KEY").ok();
            set_stripe_secret_key(value);
            Self { previous }
        }
    }

    impl Drop for StripeSecretKeyGuard {
        fn drop(&mut self) {
            set_stripe_secret_key(self.previous.as_deref());
        }
    }

    struct StripeWebhookSecretGuard {
        previous: Option<String>,
    }

    impl StripeWebhookSecretGuard {
        fn set(value: Option<&str>) -> Self {
            let previous = std::env::var("STRIPE_WEBHOOK_SECRET").ok();
            set_stripe_webhook_secret(value);
            Self { previous }
        }
    }

    impl Drop for StripeWebhookSecretGuard {
        fn drop(&mut self) {
            set_stripe_webhook_secret(self.previous.as_deref());
        }
    }

    async fn test_pool() -> SqlitePool {
        let connect_options = SqliteConnectOptions::from_str("sqlite::memory:")
            .expect("valid sqlite memory url")
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(connect_options)
            .await
            .expect("connect sqlite memory db");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");

        pool
    }

    #[tokio::test]
    async fn billing_new_panics_without_stripe_secret_key() {
        let _lock = env_lock().lock().await;
        let _secret_env = StripeSecretKeyGuard::set(None);
        let _webhook_env = StripeWebhookSecretGuard::set(Some("whsec_test_dummy"));

        let pool = test_pool().await;
        let query = Query::new(pool.clone());
        let command = Command::new(pool);
        let robot = Robot::test_stub();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Billing::new(query, command, robot)
        }));

        let panic_payload = match result {
            Ok(_) => panic!("constructor should panic when STRIPE_SECRET_KEY is missing"),
            Err(payload) => payload,
        };
        let panic_msg = if let Some(msg) = panic_payload.downcast_ref::<&str>() {
            (*msg).to_string()
        } else if let Some(msg) = panic_payload.downcast_ref::<String>() {
            msg.clone()
        } else {
            String::new()
        };

        assert!(
            panic_msg.contains("missing STRIPE_SECRET_KEY environment variable"),
            "unexpected panic: {panic_msg}"
        );
    }

    #[tokio::test]
    async fn billing_new_panics_without_stripe_webhook_secret() {
        let _lock = env_lock().lock().await;
        let _secret_env = StripeSecretKeyGuard::set(Some("sk_test_dummy"));
        let _webhook_env = StripeWebhookSecretGuard::set(None);

        let pool = test_pool().await;
        let query = Query::new(pool.clone());
        let command = Command::new(pool);
        let robot = Robot::test_stub();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Billing::new(query, command, robot)
        }));

        let panic_payload = match result {
            Ok(_) => panic!("constructor should panic when STRIPE_WEBHOOK_SECRET is missing"),
            Err(payload) => payload,
        };
        let panic_msg = if let Some(msg) = panic_payload.downcast_ref::<&str>() {
            (*msg).to_string()
        } else if let Some(msg) = panic_payload.downcast_ref::<String>() {
            msg.clone()
        } else {
            String::new()
        };

        assert!(
            panic_msg.contains("missing STRIPE_WEBHOOK_SECRET environment variable"),
            "unexpected panic: {panic_msg}"
        );
    }

    #[tokio::test]
    async fn billing_new_panics_with_blank_stripe_webhook_secret() {
        let _lock = env_lock().lock().await;
        let _secret_env = StripeSecretKeyGuard::set(Some("sk_test_dummy"));
        let _webhook_env = StripeWebhookSecretGuard::set(Some("   "));

        let pool = test_pool().await;
        let query = Query::new(pool.clone());
        let command = Command::new(pool);
        let robot = Robot::test_stub();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Billing::new(query, command, robot)
        }));

        let panic_payload = match result {
            Ok(_) => panic!("constructor should panic when STRIPE_WEBHOOK_SECRET is blank"),
            Err(payload) => payload,
        };
        let panic_msg = if let Some(msg) = panic_payload.downcast_ref::<&str>() {
            (*msg).to_string()
        } else if let Some(msg) = panic_payload.downcast_ref::<String>() {
            msg.clone()
        } else {
            String::new()
        };

        assert!(
            panic_msg.contains("missing STRIPE_WEBHOOK_SECRET environment variable"),
            "unexpected panic: {panic_msg}"
        );
    }

    #[tokio::test]
    async fn billing_new_accepts_non_empty_stripe_secrets() {
        let _lock = env_lock().lock().await;
        let _secret_env = StripeSecretKeyGuard::set(Some("sk_test_dummy"));
        let _webhook_env = StripeWebhookSecretGuard::set(Some("whsec_test_dummy"));

        let pool = test_pool().await;
        let billing = Billing::new(
            Query::new(pool.clone()),
            Command::new(pool),
            Robot::test_stub(),
        );

        assert_eq!(billing.stripe_secret_key, "sk_test_dummy");
        assert_eq!(billing.stripe_webhook_secret, "whsec_test_dummy");
    }
}
