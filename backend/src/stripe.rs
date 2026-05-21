//! A thin async wrapper around the subset of the Stripe REST API this service uses.
//!
//! Nothing here knows about relays, tenants, or our database — it just speaks HTTP
//! to Stripe and hands back `serde_json::Value` (or small typed results). The
//! domain logic lives in [`crate::billing`].

use anyhow::{Result, anyhow};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::BTreeMap;

use crate::env::Env;

const STRIPE_API: &str = "https://api.stripe.com/v1";

// Webhooks

const WEBHOOK_TOLERANCE_SECS: i64 = 300;

#[derive(serde::Deserialize)]
pub struct StripeWebhookEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: StripeWebhookEventData,
}

#[derive(serde::Deserialize)]
pub struct StripeWebhookEventData {
    pub object: serde_json::Value,
}

// API return types

#[derive(serde::Deserialize)]
pub struct StripeSubscription {
    pub id: String,
    pub status: String,
    #[serde(deserialize_with = "deserialize_list")]
    pub items: Vec<StripeSubscriptionItem>,
}

#[derive(serde::Deserialize)]
pub struct StripeSubscriptionItem {
    pub id: String,
    pub price: StripePrice,
    #[serde(default = "default_quantity")]
    pub quantity: i64,
}

#[derive(serde::Deserialize)]
pub struct StripePrice {
    pub id: String,
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
pub struct StripeInvoice {
    pub id: String,
    pub customer: String,
    pub status: String,
    pub amount_due: i64,
    pub currency: String,
}

#[derive(serde::Deserialize)]
struct StripeList<T> {
    data: Vec<T>,
}

fn deserialize_list<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Ok(<StripeList<T> as serde::Deserialize>::deserialize(deserializer)?.data)
}

fn default_quantity() -> i64 {
    1
}

// Stripe struct and impl

#[derive(Clone)]
pub struct Stripe {
    env: Env,
    http: reqwest::Client,
}

impl Stripe {
    pub fn new(env: &Env) -> Self {
        Self {
            env: env.clone(),
            http: reqwest::Client::new(),
        }
    }

    // --- Request helpers ---

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .get(format!("{STRIPE_API}{path}"))
            .bearer_auth(&self.env.stripe_secret_key)
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .post(format!("{STRIPE_API}{path}"))
            .bearer_auth(&self.env.stripe_secret_key)
    }

    fn delete(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .delete(format!("{STRIPE_API}{path}"))
            .bearer_auth(&self.env.stripe_secret_key)
    }

    fn idempotency_key(&self, parts: &[&str]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.env.stripe_secret_key.as_bytes())
            .expect("HMAC accepts any key length");
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                mac.update(b":");
            }
            mac.update(part.as_bytes());
        }
        hex::encode(mac.finalize().into_bytes())
    }

    // --- Customers ---

    pub async fn create_customer(&self, tenant_pubkey: &str, name: &str) -> Result<String> {
        let body = self
            .post("/customers")
            .header(
                "Idempotency-Key",
                self.idempotency_key(&["create_customer", tenant_pubkey]),
            )
            .form(&[("name", name), ("metadata[tenant_pubkey]", tenant_pubkey)])
            .send_json()
            .await?;
        let customer_id = body["id"]
            .as_str()
            .ok_or_else(|| anyhow!("missing customer id"))?;
        Ok(customer_id.to_string())
    }

    // --- Subscriptions ---

    pub async fn get_subscription(
        &self,
        subscription_id: &str,
    ) -> Result<Option<StripeSubscription>> {
        let body = self
            .get(&format!("/subscriptions/{subscription_id}"))
            .send_optional_json()
            .await?;
        body.map(serde_json::from_value)
            .transpose()
            .map_err(Into::into)
    }

    /// Stripe requires at least one item to create a subscription, so the desired
    /// items are sent inline here; [`crate::billing`] reconciles from there.
    pub async fn create_subscription(
        &self,
        customer_id: &str,
        items: &BTreeMap<String, i64>,
    ) -> Result<StripeSubscription> {
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

        Ok(self
            .post("/subscriptions")
            .header("Idempotency-Key", self.idempotency_key(&key_refs))
            .form(&form)
            .send_ok()
            .await?
            .json()
            .await?)
    }

    pub async fn create_subscription_item(
        &self,
        subscription_id: &str,
        price_id: &str,
        quantity: i64,
    ) -> Result<()> {
        let quantity = quantity.to_string();
        self.post("/subscription_items")
            .header(
                "Idempotency-Key",
                self.idempotency_key(&["create_subscription_item", subscription_id, price_id]),
            )
            .form(&[
                ("subscription", subscription_id),
                ("price", price_id),
                ("quantity", quantity.as_str()),
            ])
            .send_ok()
            .await?;
        Ok(())
    }

    pub async fn set_subscription_item_quantity(&self, item_id: &str, quantity: i64) -> Result<()> {
        self.post(&format!("/subscription_items/{item_id}"))
            .form(&[("quantity", quantity.to_string())])
            .send_ok()
            .await?;
        Ok(())
    }

    pub async fn delete_subscription_item(&self, item_id: &str) -> Result<()> {
        self.delete(&format!("/subscription_items/{item_id}"))
            .send_ok()
            .await?;
        Ok(())
    }

    pub async fn cancel_subscription(&self, subscription_id: &str) -> Result<()> {
        self.delete(&format!("/subscriptions/{subscription_id}"))
            .send_ok()
            .await?;
        Ok(())
    }

    // --- Invoices ---

    pub async fn list_invoices(&self, customer_id: &str) -> Result<Vec<StripeInvoice>> {
        let list: StripeList<StripeInvoice> = self
            .get("/invoices")
            .query(&[("customer", customer_id)])
            .send_ok()
            .await?
            .json()
            .await?;
        Ok(list.data)
    }

    pub async fn get_invoice(&self, invoice_id: &str) -> Result<Option<StripeInvoice>> {
        let body = self
            .get(&format!("/invoices/{invoice_id}"))
            .send_optional_json()
            .await?;
        body.map(serde_json::from_value)
            .transpose()
            .map_err(Into::into)
    }

    pub async fn pay_invoice(&self, invoice_id: &str) -> Result<()> {
        self.post(&format!("/invoices/{invoice_id}/pay"))
            .header(
                "Idempotency-Key",
                self.idempotency_key(&["pay_invoice", invoice_id]),
            )
            .send_ok()
            .await?;
        Ok(())
    }

    pub async fn pay_invoice_out_of_band(&self, invoice_id: &str) -> Result<()> {
        self.post(&format!("/invoices/{invoice_id}/pay"))
            .header(
                "Idempotency-Key",
                self.idempotency_key(&["pay_invoice_oob", invoice_id]),
            )
            .form(&[("paid_out_of_band", "true")])
            .send_ok()
            .await?;
        Ok(())
    }

    // --- Payment methods ---

    pub async fn has_payment_method(&self, customer_id: &str) -> Result<bool> {
        let body = self
            .get("/payment_methods")
            .query(&[("customer", customer_id), ("type", "card")])
            .send_json()
            .await?;
        Ok(body["data"].as_array().is_some_and(|a| !a.is_empty()))
    }

    // --- Portal ---

    pub async fn create_portal_session(
        &self,
        customer_id: &str,
        return_url: Option<&str>,
    ) -> Result<String> {
        let mut params = vec![("customer", customer_id.to_string())];
        if let Some(url) = return_url {
            params.push(("return_url", url.to_string()));
        }
        let body = self
            .post("/billing_portal/sessions")
            .form(&params)
            .send_json()
            .await?;
        body["url"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("missing portal session url"))
    }

    // --- Webhooks ---

    pub fn get_webhook_event(&self, payload: &str, signature: &str) -> Result<StripeWebhookEvent> {
        let mut timestamp = None;
        let mut sig = None;
        for part in signature.split(',') {
            if let Some(t) = part.strip_prefix("t=") {
                timestamp = Some(t);
            } else if let Some(v) = part.strip_prefix("v1=") {
                sig = Some(v);
            }
        }
        let timestamp = timestamp.ok_or_else(|| anyhow!("missing webhook timestamp"))?;
        let signature = sig.ok_or_else(|| anyhow!("missing webhook signature"))?;

        let signed_payload = format!("{timestamp}.{payload}");
        let mut mac = Hmac::<Sha256>::new_from_slice(self.env.stripe_webhook_secret.as_bytes())
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
        Ok(serde_json::from_str(payload)?)
    }

}

trait StripeRequest {
    async fn send_ok(self) -> Result<reqwest::Response>;
    async fn send_json(self) -> Result<serde_json::Value>;
    async fn send_optional_json(self) -> Result<Option<serde_json::Value>>;
}

impl StripeRequest for reqwest::RequestBuilder {
    async fn send_ok(self) -> Result<reqwest::Response> {
        error_for_status(self.send().await?).await
    }

    async fn send_json(self) -> Result<serde_json::Value> {
        Ok(self.send_ok().await?.json().await?)
    }

    async fn send_optional_json(self) -> Result<Option<serde_json::Value>> {
        let resp = self.send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Ok(Some(error_for_status(resp).await?.json().await?))
    }
}

/// Give callers an actionable message instead of a bare "400 Bad Request"
async fn error_for_status(resp: reqwest::Response) -> Result<reqwest::Response> {
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
