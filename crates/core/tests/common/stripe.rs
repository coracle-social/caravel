//! A fake of the parts of Stripe's API that memberships, payouts and card charges
//! use, holding its state in memory for tests to inspect and move along.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use caravel_adapters::stripe::Stripe;
use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub struct CheckoutSession {
    pub id: String,
    pub customer: String,
    pub membership_id: String,
    pub status: &'static str,
    pub subscription: Option<String>,
    pub form: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct Subscription {
    pub id: String,
    pub customer: String,
    pub period_end: i64,
    pub cancel_at_period_end: bool,
    pub canceled: bool,
    pub past_due: bool,
}

#[derive(Default)]
pub struct StripeState {
    next: u64,
    pub sessions: Vec<CheckoutSession>,
    pub subscriptions: Vec<Subscription>,
    pub accounts: HashMap<String, bool>,
    /// Decline every new card charge while set.
    decline_charges: bool,
    /// Each charge's answer by idempotency key, which Stripe replays.
    charges: HashMap<String, (StatusCode, Value)>,
    /// Each customer's saved card.
    cards: HashMap<String, String>,
    /// Subscriptions whose latest invoice hasn't been collected.
    unpaid_invoices: HashSet<String>,
}

impl StripeState {
    fn id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}_{}", self.next)
    }
}

type Shared = Arc<Mutex<StripeState>>;

#[derive(Clone)]
pub struct FakeStripe {
    pub state: Shared,
    url: String,
}

impl FakeStripe {
    pub async fn start() -> Self {
        let state: Shared = Arc::default();
        let router = Router::new()
            .route("/customers", post(create_customer))
            .route("/payment_intents", post(create_payment_intent))
            .route("/payment_methods", get(list_payment_methods))
            .route(
                "/checkout/sessions",
                post(create_session).get(list_sessions),
            )
            .route("/checkout/sessions/{id}", get(get_session))
            .route("/checkout/sessions/{id}/expire", post(expire_session))
            .route("/subscriptions", get(list_subscriptions))
            .route(
                "/subscriptions/{id}",
                get(get_subscription)
                    .post(update_subscription)
                    .delete(cancel_subscription),
            )
            .route("/accounts", post(create_account))
            .route("/accounts/{id}", get(get_account))
            .route("/accounts/{id}/login_links", post(login_link))
            .route("/account_links", post(account_link))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await });
        Self { state, url }
    }

    pub fn stripe(&self) -> Stripe {
        Stripe::with_api("sk_test_fake", &self.url)
    }

    fn state(&self) -> std::sync::MutexGuard<'_, StripeState> {
        self.state.lock().unwrap()
    }

    /// The checkout session most recently opened.
    pub fn last_session(&self) -> CheckoutSession {
        self.state()
            .sessions
            .last()
            .cloned()
            .expect("no checkout session")
    }

    /// Pay for a checkout session with a new subscription running until
    /// `period_end`, returning the subscription's id.
    pub fn complete(&self, session_id: &str, period_end: i64) -> String {
        let mut state = self.state();
        let subscription_id = state.id("sub");
        let session = state
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
            .expect("no such session");
        session.status = "complete";
        session.subscription = Some(subscription_id.clone());
        let customer = session.customer.clone();
        state.subscriptions.push(Subscription {
            id: subscription_id.clone(),
            customer,
            period_end,
            cancel_at_period_end: false,
            canceled: false,
            past_due: false,
        });
        subscription_id
    }

    /// Expire a checkout session, as Stripe does a day after opening it.
    pub fn expire(&self, session_id: &str) {
        let mut state = self.state();
        let session = state
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
            .expect("no such session");
        session.status = "expired";
    }

    pub fn subscription(&self, id: &str) -> Subscription {
        self.state()
            .subscriptions
            .iter()
            .find(|subscription| subscription.id == id)
            .cloned()
            .expect("no such subscription")
    }

    /// Move a subscription's period end, as a renewal or a lapse would.
    pub fn set_period_end(&self, id: &str, period_end: i64) {
        let mut state = self.state();
        let subscription = state
            .subscriptions
            .iter_mut()
            .find(|subscription| subscription.id == id)
            .expect("no such subscription");
        subscription.period_end = period_end;
    }

    /// Fail a subscription's renewal: Stripe still moves it to its next period.
    pub fn set_past_due(&self, id: &str, period_end: i64) {
        let mut state = self.state();
        let subscription = state
            .subscriptions
            .iter_mut()
            .find(|subscription| subscription.id == id)
            .expect("no such subscription");
        subscription.period_end = period_end;
        subscription.past_due = true;
        state.unpaid_invoices.insert(id.to_string());
    }

    /// Move a subscription to its next period with its invoice not yet collected,
    /// as Stripe leaves a renewal for about an hour before charging it.
    pub fn start_renewal(&self, id: &str, period_end: i64) {
        self.set_period_end(id, period_end);
        self.state().unpaid_invoices.insert(id.to_string());
    }

    /// Collect a subscription's latest invoice, putting it back in good standing.
    pub fn pay_renewal(&self, id: &str) {
        let mut state = self.state();
        state.unpaid_invoices.remove(id);
        let subscription = state
            .subscriptions
            .iter_mut()
            .find(|subscription| subscription.id == id)
            .expect("no such subscription");
        subscription.past_due = false;
    }

    pub fn set_account_ready(&self, account_id: &str, ready: bool) {
        self.state().accounts.insert(account_id.to_string(), ready);
    }

    /// Decline card charges from now on, or stop declining them.
    pub fn set_decline_charges(&self, decline: bool) {
        self.state().decline_charges = decline;
    }

    /// Save a card for a customer, as the billing portal would.
    pub fn save_card(&self, customer_id: &str, payment_method_id: &str) {
        self.state()
            .cards
            .insert(customer_id.to_string(), payment_method_id.to_string());
    }
}

fn session_json(session: &CheckoutSession) -> Value {
    json!({
        "id": session.id,
        "url": format!("https://checkout.stripe.test/{}", session.id),
        "status": session.status,
        "payment_status": if session.status == "complete" { "paid" } else { "unpaid" },
        "customer": session.customer,
        "subscription": session.subscription,
        "metadata": { "membership_id": session.membership_id },
        "expires_at": 0,
    })
}

fn subscription_json(subscription: &Subscription) -> Value {
    json!({
        "id": subscription.id,
        "status": match (subscription.canceled, subscription.past_due) {
            (true, _) => "canceled",
            (false, true) => "past_due",
            (false, false) => "active",
        },
        "cancel_at_period_end": subscription.cancel_at_period_end,
        "items": { "data": [{ "current_period_end": subscription.period_end }] },
    })
}

async fn create_customer(State(state): State<Shared>) -> Json<Value> {
    Json(json!({ "id": state.lock().unwrap().id("cus") }))
}

/// Charge a card, or decline it while `decline_charges` is set, replaying the first
/// answer given for an idempotency key as Stripe does.
async fn create_payment_intent(
    State(state): State<Shared>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let key = headers
        .get("Idempotency-Key")
        .and_then(|key| key.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let mut state = state.lock().unwrap();
    if let Some((status, body)) = state.charges.get(&key) {
        return (*status, Json(body.clone()));
    }

    let answer = if state.decline_charges {
        let error =
            json!({ "error": { "type": "card_error", "message": "Your card was declined." } });
        (StatusCode::PAYMENT_REQUIRED, error)
    } else {
        (
            StatusCode::OK,
            json!({ "id": state.id("pi"), "status": "succeeded" }),
        )
    };
    state.charges.insert(key, answer.clone());
    (answer.0, Json(answer.1))
}

async fn list_payment_methods(
    State(state): State<Shared>,
    Query(query): Query<HashMap<String, String>>,
) -> Json<Value> {
    let state = state.lock().unwrap();
    let data: Vec<Value> = query
        .get("customer")
        .and_then(|customer| state.cards.get(customer))
        .map(|id| json!({ "id": id }))
        .into_iter()
        .collect();
    Json(json!({ "data": data }))
}

async fn create_session(
    State(state): State<Shared>,
    Form(form): Form<Vec<(String, String)>>,
) -> Json<Value> {
    let form: HashMap<String, String> = form.into_iter().collect();
    let mut state = state.lock().unwrap();
    let session = CheckoutSession {
        id: state.id("cs"),
        customer: form.get("customer").cloned().unwrap_or_default(),
        membership_id: form
            .get("metadata[membership_id]")
            .cloned()
            .unwrap_or_default(),
        status: "open",
        subscription: None,
        form,
    };
    let body = session_json(&session);
    state.sessions.push(session);
    Json(body)
}

async fn list_sessions(
    State(state): State<Shared>,
    Query(query): Query<HashMap<String, String>>,
) -> Json<Value> {
    let state = state.lock().unwrap();
    let data: Vec<Value> = state
        .sessions
        .iter()
        .filter(|session| {
            Some(&session.customer) == query.get("customer") && session.status == "open"
        })
        .map(session_json)
        .collect();
    Json(json!({ "data": data }))
}

async fn get_session(State(state): State<Shared>, Path(id): Path<String>) -> Json<Value> {
    let state = state.lock().unwrap();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == id)
        .expect("no such session");
    Json(session_json(session))
}

async fn expire_session(State(state): State<Shared>, Path(id): Path<String>) -> Json<Value> {
    let mut state = state.lock().unwrap();
    let session = state
        .sessions
        .iter_mut()
        .find(|session| session.id == id)
        .expect("no such session");
    session.status = "expired";
    Json(session_json(session))
}

async fn list_subscriptions(
    State(state): State<Shared>,
    Query(query): Query<HashMap<String, String>>,
) -> Json<Value> {
    let state = state.lock().unwrap();
    // Stripe leaves canceled subscriptions out unless asked for them.
    let data: Vec<Value> = state
        .subscriptions
        .iter()
        .filter(|subscription| {
            Some(&subscription.customer) == query.get("customer") && !subscription.canceled
        })
        .map(subscription_json)
        .collect();
    Json(json!({ "data": data }))
}

async fn get_subscription(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, StatusCode> {
    let state = state.lock().unwrap();
    // The sweep also asks about other tests' memberships, which Stripe wouldn't know.
    let subscription = state
        .subscriptions
        .iter()
        .find(|subscription| subscription.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;
    let mut body = subscription_json(subscription);
    // The latest invoice is an id unless the request expands it.
    let invoice_id = format!("in_{id}");
    let expanded = query.get("expand[]").map(String::as_str) == Some("latest_invoice");
    body["latest_invoice"] = if expanded {
        let status = match (state.unpaid_invoices.contains(&id), subscription.past_due) {
            (false, _) => "paid",
            (true, true) => "open",
            (true, false) => "draft",
        };
        json!({ "id": invoice_id, "status": status })
    } else {
        json!(invoice_id)
    };
    Ok(Json(body))
}

async fn update_subscription(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> Json<Value> {
    let mut state = state.lock().unwrap();
    let subscription = state
        .subscriptions
        .iter_mut()
        .find(|subscription| subscription.id == id)
        .expect("no such subscription");
    if form.get("cancel_at_period_end").map(String::as_str) == Some("true") {
        subscription.cancel_at_period_end = true;
    }
    Json(subscription_json(subscription))
}

async fn cancel_subscription(State(state): State<Shared>, Path(id): Path<String>) -> Json<Value> {
    let mut state = state.lock().unwrap();
    let subscription = state
        .subscriptions
        .iter_mut()
        .find(|subscription| subscription.id == id)
        .expect("no such subscription");
    subscription.canceled = true;
    Json(subscription_json(subscription))
}

async fn create_account(State(state): State<Shared>) -> Json<Value> {
    let mut state = state.lock().unwrap();
    let id = state.id("acct");
    state.accounts.insert(id.clone(), false);
    Json(json!({ "id": id }))
}

async fn get_account(State(state): State<Shared>, Path(id): Path<String>) -> Json<Value> {
    let ready = state
        .lock()
        .unwrap()
        .accounts
        .get(&id)
        .copied()
        .unwrap_or(false);
    Json(json!({ "id": id, "charges_enabled": ready, "payouts_enabled": ready }))
}

async fn account_link(Form(form): Form<HashMap<String, String>>) -> Json<Value> {
    Json(json!({ "url": format!("https://connect.stripe.test/onboard/{}", form["account"]) }))
}

async fn login_link(Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "url": format!("https://connect.stripe.test/dashboard/{id}") }))
}
