//! A thin async wrapper around the subset of the Stripe REST API this service uses.
//!
//! Nothing here knows about relays, tenants, or our database — it just speaks HTTP
//! to Stripe and hands back `serde_json::Value` (or small typed results). The
//! domain logic lives in [`crate::billing`].

use anyhow::{Result, anyhow};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::env;

const STRIPE_API: &str = "https://api.stripe.com/v1";

#[derive(Clone)]
pub struct Stripe {
    http: reqwest::Client,
}

impl Default for Stripe {
    fn default() -> Self {
        Self::new()
    }
}

impl Stripe {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
        }
    }

    // --- Request helpers ---

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .get(format!("{STRIPE_API}{path}"))
            .bearer_auth(&env::get().stripe_secret_key)
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .post(format!("{STRIPE_API}{path}"))
            .bearer_auth(&env::get().stripe_secret_key)
    }

    fn idempotency_key(&self, parts: &[&str]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(env::get().stripe_secret_key.as_bytes())
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

    /// Create a Stripe customer for a tenant and return its id. Idempotent on
    /// `tenant_pubkey` so retrying a tenant's creation reuses the same customer.
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

    // --- Payment methods ---

    /// Return the id of the customer's first saved payment method, or `None` if
    /// they have none. The returned `pm_…` id can be charged off-session via
    /// [`Self::create_payment_intent`]. We don't track a Stripe "default" payment
    /// method, so the first one Stripe lists is the one we'll charge.
    pub async fn get_saved_payment_method(&self, customer_id: &str) -> Result<Option<String>> {
        let body = self
            .get("/payment_methods")
            .query(&[("customer", customer_id), ("type", "card")])
            .send_json()
            .await?;
        Ok(body["data"]
            .as_array()
            .and_then(|methods| methods.first())
            .and_then(|method| method["id"].as_str())
            .map(str::to_string))
    }

    // --- Intents ---

    /// Create and immediately confirm an off-session PaymentIntent charging a
    /// saved payment method. `amount` is in the currency's minor units (cents for
    /// `usd`). Returns the PaymentIntent id on success.
    ///
    /// A decline or an issuer authentication demand (`authentication_required`,
    /// which we can't satisfy off-session) comes back from Stripe as an HTTP
    /// error, so the caller naturally falls through to another payment method.
    /// The charge is made idempotent on `invoice_id`, so a retried collection
    /// reuses the same charge instead of billing the payment method twice.
    pub async fn create_payment_intent(
        &self,
        customer_id: &str,
        payment_method_id: &str,
        invoice_id: &str,
        amount: i64,
        currency: &str,
    ) -> Result<String> {
        let amount = amount.to_string();
        let body = self
            .post("/payment_intents")
            .header(
                "Idempotency-Key",
                self.idempotency_key(&["payment_intent", invoice_id]),
            )
            .form(&[
                ("amount", amount.as_str()),
                ("currency", currency),
                ("customer", customer_id),
                ("payment_method", payment_method_id),
                ("off_session", "true"),
                ("confirm", "true"),
            ])
            .send_json()
            .await?;

        // A successful off-session charge settles synchronously. Anything
        // else (e.g. `requires_action`) can't be completed without the customer,
        // so treat it as a failure and let the caller fall back.
        let status = body["status"].as_str().unwrap_or_default();
        if status != "succeeded" {
            return Err(anyhow!("payment intent not succeeded (status: {status})"));
        }

        body["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("missing payment intent id"))
    }

    // --- Portal ---

    /// Open a Stripe billing-portal session for the customer, returning the URL
    /// where they can manage their saved payment methods.
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
}

// Stripe request util

trait StripeRequest {
    async fn send_ok(self) -> Result<reqwest::Response>;
    async fn send_json(self) -> Result<serde_json::Value>;
}

impl StripeRequest for reqwest::RequestBuilder {
    async fn send_ok(self) -> Result<reqwest::Response> {
        error_for_status(self.send().await?).await
    }

    async fn send_json(self) -> Result<serde_json::Value> {
        Ok(self.send_ok().await?.json().await?)
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
