//! A thin async wrapper around the subset of the Stripe REST API this service uses.
//!
//! Nothing here knows about relays, tenants, or our database — it just speaks HTTP
//! to Stripe and hands back `serde_json::Value` (or small typed results). The
//! domain logic lives in [`crate::billing`].

use anyhow::{Result, anyhow};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::BTreeMap;

type HmacSha256 = Hmac<Sha256>;

const STRIPE_API: &str = "https://api.stripe.com/v1";
const WEBHOOK_TOLERANCE_SECS: i64 = 300;

/// A Stripe webhook event with its signature already verified.
#[derive(serde::Deserialize)]
pub struct Event {
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: EventData,
}

#[derive(serde::Deserialize)]
pub struct EventData {
    pub object: serde_json::Value,
}

#[derive(Clone)]
pub struct Stripe {
    pub(crate) secret_key: String,
    pub(crate) webhook_secret: String,
    http: reqwest::Client,
}

impl Stripe {
    pub fn new(secret_key: String, webhook_secret: String) -> Self {
        Self {
            secret_key,
            webhook_secret,
            http: reqwest::Client::new(),
        }
    }

    // --- Customers ---

    /// Creates a customer with the given display name, tagging it with the tenant
    /// pubkey in metadata. Idempotent on the tenant pubkey.
    pub async fn create_customer(&self, name: &str, tenant_pubkey: &str) -> Result<String> {
        let resp = self
            .http
            .post(format!("{STRIPE_API}/customers"))
            .bearer_auth(&self.secret_key)
            .header(
                "Idempotency-Key",
                self.idempotency_key(&["create_customer", tenant_pubkey]),
            )
            .form(&[("name", name), ("metadata[tenant_pubkey]", tenant_pubkey)])
            .send()
            .await?;

        let body: serde_json::Value = error_for_status(resp).await?.json().await?;
        let customer_id = body["id"]
            .as_str()
            .ok_or_else(|| anyhow!("missing customer id"))?;
        if !customer_id.starts_with("cus_") {
            return Err(anyhow!("unexpected customer id format"));
        }
        Ok(customer_id.to_string())
    }

    // --- Subscriptions ---

    /// Fetches a subscription, returning `None` if Stripe no longer knows about it
    /// (so callers can recover from a stale subscription id).
    pub async fn get_subscription(
        &self,
        subscription_id: &str,
    ) -> Result<Option<serde_json::Value>> {
        let resp = self
            .http
            .get(format!("{STRIPE_API}/subscriptions/{subscription_id}"))
            .bearer_auth(&self.secret_key)
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let body: serde_json::Value = error_for_status(resp).await?.json().await?;
        Ok(Some(body))
    }

    /// Creates a subscription with one item per `(price_id, quantity)` entry, billed
    /// automatically. Returns the subscription id and a map from price id to the
    /// created subscription item id. Idempotent on the customer and the item set.
    pub async fn create_subscription(
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

        let resp = self
            .http
            .post(format!("{STRIPE_API}/subscriptions"))
            .bearer_auth(&self.secret_key)
            .header("Idempotency-Key", self.idempotency_key(&key_refs))
            .form(&form)
            .send()
            .await?;
        let body: serde_json::Value = error_for_status(resp).await?.json().await?;

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

    pub async fn create_subscription_item(
        &self,
        subscription_id: &str,
        price_id: &str,
        quantity: i64,
    ) -> Result<String> {
        let quantity = quantity.to_string();
        let resp = self
            .http
            .post(format!("{STRIPE_API}/subscription_items"))
            .bearer_auth(&self.secret_key)
            .header(
                "Idempotency-Key",
                self.idempotency_key(&["create_subscription_item", subscription_id, price_id]),
            )
            .form(&[
                ("subscription", subscription_id),
                ("price", price_id),
                ("quantity", quantity.as_str()),
            ])
            .send()
            .await?;
        let body: serde_json::Value = error_for_status(resp).await?.json().await?;
        body["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("missing subscription item id"))
    }

    /// Sets a subscription item's quantity. No idempotency key: this is a
    /// reconcile-to-desired-state write, and re-applying the same target is a no-op.
    pub async fn set_subscription_item_quantity(&self, item_id: &str, quantity: i64) -> Result<()> {
        let resp = self
            .http
            .post(format!("{STRIPE_API}/subscription_items/{item_id}"))
            .bearer_auth(&self.secret_key)
            .form(&[("quantity", quantity.to_string())])
            .send()
            .await?;
        error_for_status(resp).await?;
        Ok(())
    }

    pub async fn delete_subscription_item(&self, item_id: &str) -> Result<()> {
        let resp = self
            .http
            .delete(format!("{STRIPE_API}/subscription_items/{item_id}"))
            .bearer_auth(&self.secret_key)
            .send()
            .await?;
        error_for_status(resp).await?;
        Ok(())
    }

    pub async fn cancel_subscription(&self, subscription_id: &str) -> Result<()> {
        let resp = self
            .http
            .delete(format!("{STRIPE_API}/subscriptions/{subscription_id}"))
            .bearer_auth(&self.secret_key)
            .send()
            .await?;
        error_for_status(resp).await?;
        Ok(())
    }

    // --- Invoices ---

    /// Returns the `data` array of the customer's invoices.
    pub async fn list_invoices(&self, customer_id: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .get(format!("{STRIPE_API}/invoices"))
            .bearer_auth(&self.secret_key)
            .query(&[("customer", customer_id)])
            .send()
            .await?;
        let body: serde_json::Value = error_for_status(resp).await?.json().await?;
        Ok(body["data"].clone())
    }

    /// Fetches an invoice, returning `None` if Stripe no longer knows about it
    /// (so callers can surface a 404 to the user).
    pub async fn get_invoice(&self, invoice_id: &str) -> Result<Option<serde_json::Value>> {
        let resp = self
            .http
            .get(format!("{STRIPE_API}/invoices/{invoice_id}"))
            .bearer_auth(&self.secret_key)
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let body: serde_json::Value = error_for_status(resp).await?.json().await?;
        Ok(Some(body))
    }

    pub async fn pay_invoice(&self, invoice_id: &str) -> Result<()> {
        let resp = self
            .http
            .post(format!("{STRIPE_API}/invoices/{invoice_id}/pay"))
            .bearer_auth(&self.secret_key)
            .header(
                "Idempotency-Key",
                self.idempotency_key(&["pay_invoice", invoice_id]),
            )
            .send()
            .await?;
        error_for_status(resp).await?;
        Ok(())
    }

    /// Marks an invoice paid out of band — used when we've collected payment over
    /// Lightning rather than through Stripe.
    pub async fn pay_invoice_out_of_band(&self, invoice_id: &str) -> Result<()> {
        let resp = self
            .http
            .post(format!("{STRIPE_API}/invoices/{invoice_id}/pay"))
            .bearer_auth(&self.secret_key)
            .header(
                "Idempotency-Key",
                self.idempotency_key(&["pay_invoice_oob", invoice_id]),
            )
            .form(&[("paid_out_of_band", "true")])
            .send()
            .await?;
        error_for_status(resp).await?;
        Ok(())
    }

    pub async fn preview_upcoming_invoice(
        &self,
        customer_id: &str,
        subscription_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut req = self
            .http
            .get(format!("{STRIPE_API}/invoices/upcoming"))
            .bearer_auth(&self.secret_key)
            .query(&[("customer", customer_id)]);
        if let Some(subscription_id) = subscription_id {
            req = req.query(&[("subscription", subscription_id)]);
        }
        let body: serde_json::Value = error_for_status(req.send().await?).await?.json().await?;
        Ok(body)
    }

    // --- Payment methods ---

    pub async fn has_payment_method(&self, customer_id: &str) -> Result<bool> {
        let resp = self
            .http
            .get(format!("{STRIPE_API}/payment_methods"))
            .bearer_auth(&self.secret_key)
            .query(&[("customer", customer_id), ("type", "card")])
            .send()
            .await?;
        let body: serde_json::Value = error_for_status(resp).await?.json().await?;
        Ok(body["data"].as_array().is_some_and(|a| !a.is_empty()))
    }

    // --- Billing portal ---

    pub async fn create_portal_session(
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
            .bearer_auth(&self.secret_key)
            .form(&params)
            .send()
            .await?;
        let body: serde_json::Value = error_for_status(resp).await?.json().await?;
        body["url"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("missing portal session url"))
    }

    // --- Webhooks ---

    /// Verifies the `Stripe-Signature` header against the configured webhook secret
    /// (including the timestamp tolerance check) and parses the event body.
    pub fn construct_event(&self, payload: &str, sig_header: &str) -> Result<Event> {
        self.verify_webhook_signature(payload, sig_header)?;
        Ok(serde_json::from_str(payload)?)
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
        let mut mac = HmacSha256::new_from_slice(self.webhook_secret.as_bytes())
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

    // --- Internals ---

    /// Derives a stable idempotency key by HMAC-ing `parts` with the secret key.
    fn idempotency_key(&self, parts: &[&str]) -> String {
        let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes())
            .expect("HMAC accepts any key length");
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                mac.update(b":");
            }
            mac.update(part.as_bytes());
        }
        hex::encode(mac.finalize().into_bytes())
    }
}

/// Like [`reqwest::Response::error_for_status`], but on a 4xx/5xx response it reads
/// the body and folds Stripe's JSON error payload (`error.message`/`code`/`param`)
/// into the returned error, so callers get an actionable message instead of a bare
/// "400 Bad Request" with only the URL.
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
