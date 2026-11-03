//! The subset of the Stripe REST API caravel uses, returning plain values.

use std::time::Duration;

use anyhow::{Result, anyhow};
use hmac::{Hmac, Mac};
use sha2::Sha256;

const STRIPE_API: &str = "https://api.stripe.com/v1";

/// Every request pins this version rather than inheriting the account's default.
const STRIPE_VERSION: &str = "2026-09-30.endive";

/// The currency everything is charged in.
const CURRENCY: &str = "usd";

/// How long a request may take, so a hung one can't stall billing.
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct Stripe {
    http: reqwest::Client,
    secret_key: String,
    api: String,
}

/// What Stripe says about a tenant's Connect account.
#[derive(Clone, Debug)]
pub struct ConnectAccount {
    pub ready: bool,
    /// Stripe's code for what it still needs, such as `requirements.past_due`.
    pub reason: Option<String>,
}

impl Stripe {
    pub fn new(secret_key: &str) -> Self {
        Self::with_api(secret_key, STRIPE_API)
    }

    /// A client of a Stripe-compatible API at `api`, such as a fake in tests.
    pub fn with_api(secret_key: &str, api: &str) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(TIMEOUT)
                .build()
                .expect("a client with a timeout builds"),
            secret_key: secret_key.to_string(),
            api: api.trim_end_matches('/').to_string(),
        }
    }

    // --- Request helpers ---

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.authed(self.http.get(format!("{}{path}", self.api)))
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.authed(self.http.post(format!("{}{path}", self.api)))
    }

    fn delete(&self, path: &str) -> reqwest::RequestBuilder {
        self.authed(self.http.delete(format!("{}{path}", self.api)))
    }

    fn authed(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request
            .bearer_auth(&self.secret_key)
            .header("Stripe-Version", STRIPE_VERSION)
    }

    fn idempotency_key(&self, parts: &[&str]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret_key.as_bytes())
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
    /// `tenant_pubkey` so retrying a tenant's creation reuses the same customer. The
    /// name is set by a second request, since Stripe refuses a retried key whose
    /// body differs and the name can change between attempts.
    pub async fn create_customer(&self, tenant_pubkey: &str, name: &str) -> Result<String> {
        let body = self
            .post("/customers")
            .header(
                "Idempotency-Key",
                self.idempotency_key(&["create_customer", tenant_pubkey]),
            )
            .form(&[("metadata[tenant_pubkey]", tenant_pubkey)])
            .send_json()
            .await?;
        let id = field(&body, "id", "customer id")?;

        self.post(&format!("/customers/{id}"))
            .form(&[("name", name)])
            .send_ok()
            .await?;
        Ok(id)
    }

    // --- Payment methods ---

    /// The customer's first saved card, which is the one charged, or `None`.
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

    /// Charge `amount` cents to a saved card off-session for an invoice. Idempotent
    /// on `attempt_id`: Stripe replays an attempt's first answer, refusals included,
    /// so retrying an attempt can't charge twice and a new attempt charges afresh.
    /// Fails when it can't be known whether the card was charged.
    pub async fn create_payment_intent(
        &self,
        customer_id: &str,
        payment_method_id: &str,
        attempt_id: &str,
        invoice_id: &str,
        amount: i64,
    ) -> Result<Charge> {
        let amount = amount.to_string();
        let resp = self
            .post("/payment_intents")
            .header(
                "Idempotency-Key",
                self.idempotency_key(&["payment_intent", attempt_id]),
            )
            .form(&[
                ("amount", amount.as_str()),
                ("currency", CURRENCY),
                ("customer", customer_id),
                ("payment_method", payment_method_id),
                ("metadata[invoice_id]", invoice_id),
                ("off_session", "true"),
                ("confirm", "true"),
            ])
            .send()
            .await?;

        // A 4xx charged nothing, unless it is a 409 for a request still in flight or a 429.
        let status = resp.status();
        let refused = status.is_client_error()
            && status != reqwest::StatusCode::CONFLICT
            && status != reqwest::StatusCode::TOO_MANY_REQUESTS;
        let body: serde_json::Value = match error_for_status(resp).await {
            Ok(resp) => resp.json().await?,
            Err(error) if refused => return Ok(Charge::Refused(error.to_string())),
            Err(error) => return Err(error),
        };

        match body["status"].as_str().unwrap_or_default() {
            "succeeded" => Ok(Charge::Succeeded(field(&body, "id", "payment intent id")?)),
            "processing" => Err(anyhow!("payment intent still processing")),
            // Anything else, such as `requires_action`, needs the customer.
            status => Ok(Charge::Refused(format!(
                "payment intent not succeeded (status: {status})"
            ))),
        }
    }

    // --- Checkout ---

    /// Open a Checkout session paying `amount` cents for one invoice on-session,
    /// which can clear a 3D Secure challenge an off-session charge can't. It and
    /// the PaymentIntent it creates carry the invoice's id.
    pub async fn create_checkout_session(
        &self,
        customer_id: &str,
        invoice_id: &str,
        amount: i64,
        success_url: &str,
        cancel_url: &str,
    ) -> Result<CheckoutSession> {
        let amount = amount.to_string();
        let body = self
            .post("/checkout/sessions")
            .form(&[
                ("mode", "payment"),
                ("customer", customer_id),
                ("success_url", success_url),
                ("cancel_url", cancel_url),
                ("line_items[0][quantity]", "1"),
                ("line_items[0][price_data][currency]", CURRENCY),
                ("line_items[0][price_data][unit_amount]", amount.as_str()),
                (
                    "line_items[0][price_data][product_data][name]",
                    "Relay subscription",
                ),
                ("payment_intent_data[metadata][invoice_id]", invoice_id),
                ("metadata[invoice_id]", invoice_id),
            ])
            .send_json()
            .await?;

        Ok(CheckoutSession {
            id: field(&body, "id", "checkout session id")?,
            url: field(&body, "url", "checkout session url")?,
            expires_at: body["expires_at"]
                .as_i64()
                .ok_or_else(|| anyhow!("missing checkout session expiry"))?,
        })
    }

    /// Whether a Checkout session has been paid.
    pub async fn is_checkout_paid(&self, session_id: &str) -> Result<bool> {
        let body = self
            .get(&format!("/checkout/sessions/{session_id}"))
            .send_json()
            .await?;
        Ok(body["payment_status"].as_str() == Some("paid"))
    }

    /// Expire a Checkout session so it can't be completed, which fails for one
    /// that isn't open.
    pub async fn expire_checkout_session(&self, session_id: &str) -> Result<()> {
        self.post(&format!("/checkout/sessions/{session_id}/expire"))
            .send_ok()
            .await?;
        Ok(())
    }

    // --- Connect ---

    /// Create an Express Connect account a tenant's members pay into, and return
    /// its id. Idempotent on `tenant_pubkey`, so a retried onboarding never
    /// leaves a tenant with two.
    pub async fn create_connect_account(&self, tenant_pubkey: &str) -> Result<String> {
        let body = self
            .post("/accounts")
            .header(
                "Idempotency-Key",
                self.idempotency_key(&["create_connect_account", tenant_pubkey]),
            )
            .form(&[
                ("type", "express"),
                ("capabilities[card_payments][requested]", "true"),
                ("capabilities[transfers][requested]", "true"),
                ("metadata[tenant_pubkey]", tenant_pubkey),
            ])
            .send_json()
            .await?;
        field(&body, "id", "connect account id")
    }

    /// What Stripe says about a Connect account: whether it can both take charges
    /// and receive payouts, and its reason if it can't.
    pub async fn connect_account(&self, account_id: &str) -> Result<ConnectAccount> {
        let body = self
            .get(&format!("/accounts/{account_id}"))
            .send_json()
            .await?;
        let ready = body["charges_enabled"].as_bool().unwrap_or(false)
            && body["payouts_enabled"].as_bool().unwrap_or(false);
        Ok(ConnectAccount {
            ready,
            reason: body["requirements"]["disabled_reason"]
                .as_str()
                .map(str::to_string),
        })
    }

    /// Open a single-use onboarding link for an Express account.
    pub async fn create_account_link(
        &self,
        account_id: &str,
        return_url: &str,
        refresh_url: &str,
    ) -> Result<String> {
        let body = self
            .post("/account_links")
            .form(&[
                ("account", account_id),
                ("type", "account_onboarding"),
                ("return_url", return_url),
                ("refresh_url", refresh_url),
            ])
            .send_json()
            .await?;
        field(&body, "url", "account link url")
    }

    /// Open a login link to an account's Express dashboard, where its payouts are.
    pub async fn create_account_login_link(&self, account_id: &str) -> Result<String> {
        let body = self
            .post(&format!("/accounts/{account_id}/login_links"))
            .send_json()
            .await?;
        field(&body, "url", "login link url")
    }

    // --- Memberships ---

    /// Create the customer one membership pays as, idempotent on the membership,
    /// so cancelling one membership's subscription can't reach another's.
    pub async fn create_member_customer(
        &self,
        membership_id: &str,
        relay_id: &str,
    ) -> Result<String> {
        let body = self
            .post("/customers")
            .header(
                "Idempotency-Key",
                self.idempotency_key(&["create_member_customer", membership_id]),
            )
            .form(&[
                ("metadata[membership_id]", membership_id),
                ("metadata[relay_id]", relay_id),
            ])
            .send_json()
            .await?;
        field(&body, "id", "customer id")
    }

    /// Open a Checkout session subscribing a membership's customer. The charge is
    /// the platform's and transfers to the tenant's Connect account less the
    /// platform's fee. A `billing_cycle_anchor` delays the first charge to it.
    pub async fn create_membership_checkout(
        &self,
        params: &MembershipCheckoutParams<'_>,
    ) -> Result<String> {
        let amount = params.amount.to_string();
        let fee = params.application_fee_percent.to_string();
        let anchor = params.billing_cycle_anchor.map(|anchor| anchor.to_string());

        let mut form = vec![
            ("mode", "subscription"),
            ("customer", params.customer_id),
            ("success_url", params.success_url),
            ("cancel_url", params.cancel_url),
            ("line_items[0][quantity]", "1"),
            ("line_items[0][price_data][currency]", CURRENCY),
            ("line_items[0][price_data][unit_amount]", &amount),
            (
                "line_items[0][price_data][recurring][interval]",
                params.interval,
            ),
            (
                "line_items[0][price_data][product_data][name]",
                params.product_name,
            ),
            (
                "subscription_data[transfer_data][destination]",
                params.destination_account_id,
            ),
            ("subscription_data[application_fee_percent]", &fee),
            ("subscription_data[metadata][relay_id]", params.relay_id),
            (
                "subscription_data[metadata][membership_id]",
                params.membership_id,
            ),
            ("metadata[relay_id]", params.relay_id),
            ("metadata[membership_id]", params.membership_id),
        ];
        if let Some(anchor) = &anchor {
            form.push(("subscription_data[billing_cycle_anchor]", anchor));
            form.push(("subscription_data[proration_behavior]", "none"));
        }

        let body = self
            .post("/checkout/sessions")
            .form(&form)
            .send_json()
            .await?;
        field(&body, "url", "checkout session url")
    }

    /// Where a membership's Checkout session stands.
    pub async fn get_membership_checkout(&self, session_id: &str) -> Result<MembershipCheckout> {
        let body = self
            .get(&format!("/checkout/sessions/{session_id}"))
            .send_json()
            .await?;
        Ok(MembershipCheckout {
            complete: body["status"].as_str() == Some("complete")
                && matches!(
                    body["payment_status"].as_str(),
                    Some("paid" | "no_payment_required")
                ),
            customer_id: body["customer"].as_str().map(str::to_string),
            subscription_id: body["subscription"].as_str().map(str::to_string),
            membership_id: body["metadata"]["membership_id"]
                .as_str()
                .map(str::to_string),
        })
    }

    /// The open Checkout sessions of a customer.
    pub async fn list_open_checkout_sessions(&self, customer_id: &str) -> Result<Vec<String>> {
        let body = self
            .get("/checkout/sessions")
            .query(&[
                ("customer", customer_id),
                ("status", "open"),
                ("limit", "100"),
            ])
            .send_json()
            .await?;
        Ok(body["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|session| session["id"].as_str().map(str::to_string))
            .collect())
    }

    /// When a subscription's current period ends, whether it stops then, where it
    /// stands, and whether its latest invoice is paid.
    pub async fn get_subscription(&self, subscription_id: &str) -> Result<Subscription> {
        let body = self
            .get(&format!("/subscriptions/{subscription_id}"))
            .query(&[("expand[]", "latest_invoice")])
            .send_json()
            .await?;
        Ok(Subscription {
            current_period_end: body["items"]["data"][0]["current_period_end"].as_i64(),
            cancel_at_period_end: body["cancel_at_period_end"].as_bool().unwrap_or(false),
            standing: match body["status"].as_str() {
                Some("active" | "trialing") => Standing::Good,
                Some("past_due") => Standing::PastDue,
                _ => Standing::Stopped,
            },
            latest_invoice_paid: body["latest_invoice"]["status"].as_str() == Some("paid"),
        })
    }

    /// A customer's subscriptions, leaving out those that never started.
    pub async fn list_live_subscriptions(&self, customer_id: &str) -> Result<Vec<String>> {
        let body = self
            .get("/subscriptions")
            .query(&[("customer", customer_id), ("limit", "100")])
            .send_json()
            .await?;
        Ok(body["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|subscription| subscription["status"].as_str() != Some("incomplete_expired"))
            .filter_map(|subscription| subscription["id"].as_str().map(str::to_string))
            .collect())
    }

    /// Stop a subscription renewing, leaving its paid period running.
    pub async fn cancel_subscription_at_period_end(&self, subscription_id: &str) -> Result<()> {
        self.post(&format!("/subscriptions/{subscription_id}"))
            .form(&[("cancel_at_period_end", "true")])
            .send_ok()
            .await?;
        Ok(())
    }

    /// End a subscription now.
    pub async fn cancel_subscription(&self, subscription_id: &str) -> Result<()> {
        self.delete(&format!("/subscriptions/{subscription_id}"))
            .send_ok()
            .await?;
        Ok(())
    }

    // --- Billing portal ---

    /// Open a billing portal session where the customer manages their saved cards,
    /// returning its url.
    pub async fn create_billing_portal_session(
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
        field(&body, "url", "billing portal session url")
    }
}

/// What a membership's checkout asks Stripe for.
pub struct MembershipCheckoutParams<'a> {
    pub customer_id: &'a str,
    pub destination_account_id: &'a str,
    pub relay_id: &'a str,
    pub membership_id: &'a str,
    pub product_name: &'a str,
    /// In cents.
    pub amount: i64,
    /// `month` or `year`.
    pub interval: &'a str,
    pub application_fee_percent: f64,
    pub billing_cycle_anchor: Option<i64>,
    pub success_url: &'a str,
    pub cancel_url: &'a str,
}

/// How an off-session charge ended, when Stripe's answer settles it.
pub enum Charge {
    /// The card was charged, by this PaymentIntent.
    Succeeded(String),
    /// Nothing was charged, for this reason.
    Refused(String),
}

/// An invoice's Checkout session.
pub struct CheckoutSession {
    pub id: String,
    pub url: String,
    pub expires_at: i64,
}

pub struct MembershipCheckout {
    /// Completed and paid for.
    pub complete: bool,
    pub customer_id: Option<String>,
    pub subscription_id: Option<String>,
    pub membership_id: Option<String>,
}

pub struct Subscription {
    pub current_period_end: Option<i64>,
    pub cancel_at_period_end: bool,
    pub standing: Standing,
    /// A renewal's invoice stays a draft for about an hour, then is open until
    /// Stripe collects it.
    pub latest_invoice_paid: bool,
}

/// Where a subscription stands with Stripe. Stripe moves a subscription to its next
/// period before collecting for it, so a period running doesn't mean it was paid for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Active or trialing.
    Good,
    /// A payment failed and Stripe is retrying it.
    PastDue,
    /// Any other status, such as unpaid or canceled: Stripe has stopped collecting.
    Stopped,
}

/// A string field of a response, which Stripe always sends.
fn field(body: &serde_json::Value, key: &str, what: &str) -> Result<String> {
    body[key]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("missing {what}"))
}

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

/// A failed response as an error carrying Stripe's own message.
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
