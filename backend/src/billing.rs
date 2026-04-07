use anyhow::{Result, anyhow};
use hmac::{Hmac, Mac};
use nwc::prelude::{
    MakeInvoiceRequest, NostrWalletConnectURI, PayInvoiceRequest as NwcPayInvoiceRequest, NWC,
};
use sha2::Sha256;

use crate::command::Command;
use crate::models::Activity;
use crate::query::Query;
use crate::robot::Robot;

type HmacSha256 = Hmac<Sha256>;

const STRIPE_API: &str = "https://api.stripe.com/v1";
const WEBHOOK_TOLERANCE_SECS: i64 = 300;

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
        let stripe_webhook_secret = std::env::var("STRIPE_WEBHOOK_SECRET").unwrap_or_default();
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
            "create_relay" | "update_relay" | "activate_relay" | "deactivate_relay"
                | "fail_relay_sync" | "complete_relay_sync"
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

        // Free plan: remove subscription item if exists, then clean up
        if relay.plan == "free" {
            if let Some(ref item_id) = relay.stripe_subscription_item_id {
                self.stripe_delete_subscription_item(item_id).await?;
                self.command.delete_relay_subscription_item(&relay.id).await?;
            }
            self.cleanup_empty_subscription(&tenant.pubkey).await?;
            return Ok(());
        }

        // Inactive relay: remove subscription item if exists, then clean up
        if relay.status == "inactive" {
            if let Some(ref item_id) = relay.stripe_subscription_item_id {
                self.stripe_delete_subscription_item(item_id).await?;
                self.command.delete_relay_subscription_item(&relay.id).await?;
            }
            self.cleanup_empty_subscription(&tenant.pubkey).await?;
            return Ok(());
        }

        // Active relay on a paid plan
        let plan = Query::list_plans()
            .into_iter()
            .find(|p| p.id == relay.plan);

        let Some(plan) = plan else {
            return Ok(());
        };

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
            self.stripe_update_subscription_item(existing_item_id, stripe_price_id)
                .await?
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
            self.command.clear_tenant_subscription(tenant_pubkey).await?;
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
                let invoice_id = obj["id"].as_str().unwrap_or_default();
                self.handle_invoice_created(customer, amount_due, invoice_id)
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

        let ts: i64 = timestamp.parse().map_err(|_| anyhow!("bad webhook timestamp"))?;
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

        // 1. NWC auto-pay: if the tenant has a nwc_url
        if !tenant.nwc_url.is_empty() {
            match self.nwc_pay_invoice(amount_due, &tenant.nwc_url).await {
                Ok(()) => {
                    self.stripe_pay_invoice_out_of_band(invoice_id).await?;
                    self.command
                        .clear_tenant_nwc_error(&tenant.pubkey)
                        .await?;
                    return Ok(());
                }
                Err(e) => {
                    let error_msg = format!("{e}");
                    self.command
                        .set_tenant_nwc_error(&tenant.pubkey, &error_msg)
                        .await?;
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
        self.robot
            .send_dm(
                &tenant.pubkey,
                "Payment is due for your relay subscription. Please visit the application to complete a manual Lightning payment.",
            )
            .await?;

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
                if relay.status == "inactive" && relay.plan != "free" {
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
            if relay.status == "active" && relay.plan != "free" {
                self.command.deactivate_relay(&relay).await?;
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
            if relay.status == "active" && relay.plan != "free" {
                self.command.deactivate_relay(&relay).await?;
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

    // --- Public API helpers ---

    pub async fn get_invoice_with_tenant(
        &self,
        invoice_id: &str,
    ) -> Result<(serde_json::Value, crate::models::Tenant)> {
        let invoice = self.stripe_get_invoice(invoice_id).await?;
        let customer_id = invoice["customer"]
            .as_str()
            .ok_or_else(|| anyhow!("invoice missing customer"))?;
        let tenant = self
            .query
            .get_tenant_by_stripe_customer_id(customer_id)
            .await?
            .ok_or_else(|| anyhow!("tenant not found for customer"))?;
        Ok((invoice, tenant))
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

    pub async fn stripe_get_invoice(&self, invoice_id: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .get(format!("{STRIPE_API}/invoices/{invoice_id}"))
            .bearer_auth(&self.stripe_secret_key)
            .send()
            .await?;

        let body: serde_json::Value = resp.error_for_status()?.json().await?;
        Ok(body)
    }

    pub async fn create_bolt11(&self, amount_due_cents: i64) -> Result<String> {
        let amount_msats = (amount_due_cents as u64) * 1000; // placeholder conversion

        let system_uri: NostrWalletConnectURI = self.nwc_url.parse()
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

    async fn stripe_create_subscription(
        &self,
        customer_id: &str,
        price_id: &str,
    ) -> Result<(String, String)> {
        let resp = self
            .http
            .post(format!("{STRIPE_API}/subscriptions"))
            .bearer_auth(&self.stripe_secret_key)
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
        let resp = self
            .http
            .post(format!("{STRIPE_API}/subscription_items"))
            .bearer_auth(&self.stripe_secret_key)
            .form(&[
                ("subscription", subscription_id),
                ("price", price_id),
            ])
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
        let resp = self
            .http
            .post(format!("{STRIPE_API}/subscription_items/{item_id}"))
            .bearer_auth(&self.stripe_secret_key)
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

    async fn stripe_pay_invoice_out_of_band(&self, invoice_id: &str) -> Result<()> {
        self.http
            .post(format!("{STRIPE_API}/invoices/{invoice_id}/pay"))
            .bearer_auth(&self.stripe_secret_key)
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

    async fn nwc_pay_invoice(&self, amount_due_cents: i64, tenant_nwc_url: &str) -> Result<()> {
        // Convert USD cents to millisatoshis (approximate: 1 sat ≈ variable USD)
        // amount_due is in cents from Stripe. We create a Lightning invoice for the exact amount.
        // The NWC make_invoice amount is in millisatoshis.
        let amount_msats = (amount_due_cents as u64) * 1000; // placeholder conversion, actual rate would come from exchange

        // Create a bolt11 invoice using the system wallet (self.nwc_url)
        let system_uri: NostrWalletConnectURI = self.nwc_url.parse()
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
        let tenant_uri: NostrWalletConnectURI = tenant_nwc_url.parse()
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
}
