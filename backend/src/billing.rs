use anyhow::{Result, anyhow};
use hmac::{Hmac, Mac};
use nwc::prelude::{
    MakeInvoiceRequest, NWC, NostrWalletConnectURI, PayInvoiceRequest as NwcPayInvoiceRequest,
};
use sha2::Sha256;

use crate::command::Command;
use crate::models::{
    Activity, RELAY_STATUS_ACTIVE, RELAY_STATUS_DELINQUENT, RELAY_STATUS_INACTIVE, Relay,
};
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

        loop {
            match rx.recv().await {
                Ok(activity) => {
                    if let Err(e) = self.handle_activity(&activity).await {
                        tracing::error!(error = %e, "billing handle_activity failed");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(missed = n, "billing lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
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

        let Some(tenant) = self.query.get_tenant(&relay.tenant).await? else {
            return Ok(());
        };

        let plan = Query::get_plan(&relay.plan)
            .ok_or_else(|| anyhow!("unknown relay plan id: {}", relay.plan))?;

        // Free plan: remove subscription item if exists, then clean up
        if plan.id == "free" {
            if let Some(ref item_id) = relay.stripe_subscription_item_id {
                self.stripe_delete_subscription_item(item_id).await?;
                self.command
                    .delete_relay_subscription_item(&relay.id)
                    .await?;
                self.validate_downgrade_proration(&tenant, "free-plan-downgrade")
                    .await;
            }
            self.cleanup_empty_subscription(&tenant.pubkey).await?;
            return Ok(());
        }

        // Inactive relay: remove subscription item if exists, then clean up
        if relay.status == RELAY_STATUS_INACTIVE || relay.status == RELAY_STATUS_DELINQUENT {
            if let Some(ref item_id) = relay.stripe_subscription_item_id {
                self.stripe_delete_subscription_item(item_id).await?;
                self.command
                    .delete_relay_subscription_item(&relay.id)
                    .await?;
            }
            self.cleanup_empty_subscription(&tenant.pubkey).await?;
            return Ok(());
        }

        // Active relay on a paid plan
        let Some(ref stripe_price_id) = plan.stripe_price_id else {
            return Ok(());
        };

        // Ensure subscription exists
        if tenant.stripe_subscription_id.is_none() {
            let (subscription_id, item_id) = self
                .stripe_create_subscription(&tenant.stripe_customer_id, stripe_price_id)
                .await?;
            self.command
                .set_tenant_subscription(&tenant.pubkey, &subscription_id)
                .await?;
            self.command
                .set_relay_subscription_item(&relay.id, &item_id)
                .await?;
            return Ok(());
        }

        // Sync the subscription item: create or update
        let subscription_id = tenant.stripe_subscription_id.as_ref().unwrap();
        let item_id = if let Some(ref existing_item_id) = relay.stripe_subscription_item_id {
            let is_downgrade = self
                .is_subscription_item_downgrade(existing_item_id, plan.amount)
                .await
                .unwrap_or_else(|error| {
                    tracing::warn!(
                        error = %error,
                        relay_id = %relay.id,
                        "failed to determine relay plan downgrade direction"
                    );
                    false
                });

            let updated_item_id = self
                .stripe_update_subscription_item(existing_item_id, stripe_price_id)
                .await?;

            if is_downgrade {
                self.validate_downgrade_proration(&tenant, "paid-plan-downgrade")
                    .await;
            }

            updated_item_id
        } else {
            self.stripe_create_subscription_item(subscription_id, stripe_price_id)
                .await?
        };
        self.command
            .set_relay_subscription_item(&relay.id, &item_id)
            .await?;

        Ok(())
    }

    async fn cleanup_empty_subscription(&self, tenant_pubkey: &str) -> Result<()> {
        let has_paid = self.query.has_active_paid_relays(tenant_pubkey).await?;
        if has_paid {
            return Ok(());
        }

        let Some(tenant) = self.query.get_tenant(tenant_pubkey).await? else {
            return Ok(());
        };

        if let Some(ref subscription_id) = tenant.stripe_subscription_id {
            self.stripe_cancel_subscription(subscription_id).await?;
            self.command
                .clear_tenant_subscription(tenant_pubkey)
                .await?;
        }

        Ok(())
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
            match self
                .nwc_pay_invoice(amount_due, currency, &tenant.nwc_url)
                .await
            {
                Ok(()) => {
                    self.stripe_pay_invoice_out_of_band(invoice_id).await?;
                    self.command.clear_tenant_nwc_error(&tenant.pubkey).await?;
                    return Ok(());
                }
                Err(e) => {
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

    async fn is_subscription_item_downgrade(
        &self,
        item_id: &str,
        next_plan_amount: i64,
    ) -> Result<bool> {
        let Some(current_price_id) = self.stripe_get_subscription_item_price_id(item_id).await?
        else {
            return Ok(false);
        };

        let Some(current_plan_amount) = Self::plan_amount_from_price_id(&current_price_id) else {
            return Ok(false);
        };

        Ok(next_plan_amount < current_plan_amount)
    }

    fn plan_amount_from_price_id(price_id: &str) -> Option<i64> {
        Query::list_plans().into_iter().find_map(|plan| {
            if plan.stripe_price_id.as_deref() == Some(price_id) {
                Some(plan.amount)
            } else {
                None
            }
        })
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

    pub async fn stripe_create_customer(&self, tenant_pubkey: &str) -> Result<String> {
        let short_pubkey: String = tenant_pubkey.chars().take(12).collect();
        let display_name = format!("Caravel tenant {short_pubkey}");
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

        let body: serde_json::Value = resp.error_for_status()?.json().await?;
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

        let body: serde_json::Value = resp.error_for_status()?.json().await?;
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

        let body: serde_json::Value = resp.error_for_status()?.json().await?;
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
                .nwc_pay_invoice(amount_due, currency, &tenant.nwc_url)
                .await
            {
                Ok(()) => {
                    if let Err(e) = self.stripe_pay_invoice_out_of_band(invoice_id).await {
                        tracing::error!(
                            error = %e,
                            invoice_id,
                            "failed to mark invoice paid out of band"
                        );
                    } else {
                        let _ = self.command.clear_tenant_nwc_error(&tenant.pubkey).await;
                    }
                }
                Err(e) => {
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

    pub async fn stripe_create_portal_session(&self, customer_id: &str) -> Result<String> {
        let resp = self
            .http
            .post(format!("{STRIPE_API}/billing_portal/sessions"))
            .bearer_auth(&self.stripe_secret_key)
            .form(&[("customer", customer_id)])
            .send()
            .await?;

        let body: serde_json::Value = resp.error_for_status()?.json().await?;
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

    async fn stripe_create_subscription(
        &self,
        customer_id: &str,
        price_id: &str,
    ) -> Result<(String, String)> {
        let idempotency_key =
            self.idempotency_key(&["create_subscription", customer_id, price_id]);
        let resp = self
            .http
            .post(format!("{STRIPE_API}/subscriptions"))
            .bearer_auth(&self.stripe_secret_key)
            .header("Idempotency-Key", idempotency_key)
            .form(&[
                ("customer", customer_id),
                ("collection_method", "charge_automatically"),
                ("items[0][price]", price_id),
            ])
            .send()
            .await?;

        let body: serde_json::Value = resp.error_for_status()?.json().await?;
        let subscription_id = body["id"]
            .as_str()
            .ok_or_else(|| anyhow!("missing subscription id"))?
            .to_string();
        let item_id = body["items"]["data"][0]["id"]
            .as_str()
            .ok_or_else(|| anyhow!("missing subscription item id"))?
            .to_string();

        Ok((subscription_id, item_id))
    }

    async fn stripe_create_subscription_item(
        &self,
        subscription_id: &str,
        price_id: &str,
    ) -> Result<String> {
        let idempotency_key =
            self.idempotency_key(&["create_subscription_item", subscription_id, price_id]);
        let resp = self
            .http
            .post(format!("{STRIPE_API}/subscription_items"))
            .bearer_auth(&self.stripe_secret_key)
            .header("Idempotency-Key", idempotency_key)
            .form(&[("subscription", subscription_id), ("price", price_id)])
            .send()
            .await?;

        let body: serde_json::Value = resp.error_for_status()?.json().await?;
        let item_id = body["id"]
            .as_str()
            .ok_or_else(|| anyhow!("missing subscription item id"))?
            .to_string();

        Ok(item_id)
    }

    async fn stripe_update_subscription_item(
        &self,
        item_id: &str,
        price_id: &str,
    ) -> Result<String> {
        let idempotency_key =
            self.idempotency_key(&["update_subscription_item", item_id, price_id]);
        let resp = self
            .http
            .post(format!("{STRIPE_API}/subscription_items/{item_id}"))
            .bearer_auth(&self.stripe_secret_key)
            .header("Idempotency-Key", idempotency_key)
            .form(&[("price", price_id)])
            .send()
            .await?;

        let body: serde_json::Value = resp.error_for_status()?.json().await?;
        let id = body["id"]
            .as_str()
            .ok_or_else(|| anyhow!("missing subscription item id"))?
            .to_string();

        Ok(id)
    }

    async fn stripe_delete_subscription_item(&self, item_id: &str) -> Result<()> {
        self.http
            .delete(format!("{STRIPE_API}/subscription_items/{item_id}"))
            .bearer_auth(&self.stripe_secret_key)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    async fn stripe_cancel_subscription(&self, subscription_id: &str) -> Result<()> {
        self.http
            .delete(format!("{STRIPE_API}/subscriptions/{subscription_id}"))
            .bearer_auth(&self.stripe_secret_key)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    async fn stripe_pay_invoice(&self, invoice_id: &str) -> Result<()> {
        let idempotency_key = self.idempotency_key(&["pay_invoice", invoice_id]);
        self.http
            .post(format!("{STRIPE_API}/invoices/{invoice_id}/pay"))
            .bearer_auth(&self.stripe_secret_key)
            .header("Idempotency-Key", idempotency_key)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    async fn stripe_get_subscription_item_price_id(&self, item_id: &str) -> Result<Option<String>> {
        let resp = self
            .http
            .get(format!("{STRIPE_API}/subscription_items/{item_id}"))
            .bearer_auth(&self.stripe_secret_key)
            .send()
            .await?;

        let body: serde_json::Value = resp.error_for_status()?.json().await?;
        Ok(body["price"]["id"].as_str().map(ToString::to_string))
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

        let body: serde_json::Value = req.send().await?.error_for_status()?.json().await?;
        Ok(body)
    }

    async fn stripe_pay_invoice_out_of_band(&self, invoice_id: &str) -> Result<()> {
        let idempotency_key = self.idempotency_key(&["pay_invoice_oob", invoice_id]);
        self.http
            .post(format!("{STRIPE_API}/invoices/{invoice_id}/pay"))
            .bearer_auth(&self.stripe_secret_key)
            .header("Idempotency-Key", idempotency_key)
            .form(&[("paid_out_of_band", "true")])
            .send()
            .await?
            .error_for_status()?;

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

        let body: serde_json::Value = resp.error_for_status()?.json().await?;
        let has_method = body["data"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false);

        Ok(has_method)
    }

    // --- NWC helpers ---

    async fn nwc_pay_invoice(
        &self,
        amount_due_minor: i64,
        currency: &str,
        tenant_nwc_url: &str,
    ) -> Result<()> {
        let amount_msats = self.fiat_minor_to_msats(amount_due_minor, currency).await?;

        // Create a bolt11 invoice using the system wallet (self.nwc_url)
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

        // Pay the bolt11 invoice using the tenant's wallet
        let tenant_uri: NostrWalletConnectURI = tenant_nwc_url
            .parse()
            .map_err(|_| anyhow!("invalid tenant NWC URL"))?;
        let tenant_nwc = NWC::new(tenant_uri);

        let pay_req = NwcPayInvoiceRequest::new(invoice_response.invoice);

        tenant_nwc
            .pay_invoice(pay_req)
            .await
            .map_err(|e| anyhow!("failed to pay invoice: {e}"))?;

        tenant_nwc.shutdown().await;

        Ok(())
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
