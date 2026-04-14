use std::sync::Arc;

use anyhow::anyhow;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::Engine;
use nostr_sdk::{Event, JsonUtil, Kind};
use serde::{Deserialize, Serialize};

use crate::billing::Billing;
use crate::command::Command;
use crate::models::{
    RELAY_STATUS_ACTIVE, RELAY_STATUS_DELINQUENT, RELAY_STATUS_INACTIVE, Relay, Tenant,
};
use crate::query::Query;
use axum::body::Bytes;

#[derive(Clone)]
pub struct Api {
    host: String,
    admins: Vec<String>,
    query: Query,
    command: Command,
    billing: Billing,
}

async fn stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let signature = headers
        .get("Stripe-Signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let payload = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(_) => return err(StatusCode::BAD_REQUEST, "bad-request", "invalid payload"),
    };

    match state.api.billing.handle_webhook(payload, signature).await {
        Ok(()) => ok(StatusCode::OK, ()),
        Err(e) => err(StatusCode::BAD_REQUEST, "webhook-error", &e.to_string()),
    }
}

#[derive(Clone)]
struct AppState {
    api: Arc<Api>,
}

#[derive(Serialize)]
struct OkResponse<T: Serialize> {
    data: T,
    code: &'static str,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    code: String,
}

#[derive(Debug)]
enum ApiError {
    Unauthorized(anyhow::Error),
    Forbidden(&'static str),
    NotFound(&'static str),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthorized(e) => err(StatusCode::UNAUTHORIZED, "unauthorized", &e.to_string()),
            Self::Forbidden(message) => err(StatusCode::FORBIDDEN, "forbidden", message),
            Self::NotFound(message) => err(StatusCode::NOT_FOUND, "not-found", message),
            Self::Internal(message) => err(StatusCode::INTERNAL_SERVER_ERROR, "internal", &message),
        }
    }
}

impl Api {
    pub fn new(query: Query, command: Command, billing: Billing) -> Self {
        let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let admins = std::env::var("ADMINS")
            .unwrap_or_default()
            .split(',')
            .map(|v| v.trim().to_lowercase())
            .filter(|v| !v.is_empty())
            .collect();
        Self {
            host,
            admins,
            query,
            command,
            billing,
        }
    }

    pub fn router(self) -> Router {
        let state = AppState {
            api: Arc::new(self),
        };

        Router::new()
            .route("/identity", get(get_identity))
            .route("/plans", get(list_plans))
            .route("/plans/:id", get(get_plan))
            .route("/tenants", get(list_tenants))
            .route("/tenants/:pubkey", get(get_tenant).put(update_tenant))
            .route("/tenants/:pubkey/relays", get(list_tenant_relays))
            .route("/relays", get(list_relays).post(create_relay))
            .route("/relays/:id", get(get_relay).put(update_relay))
            .route("/relays/:id/activity", get(list_relay_activity))
            .route("/relays/:id/deactivate", post(deactivate_relay))
            .route("/relays/:id/reactivate", post(reactivate_relay))
            .route("/tenants/:pubkey/invoices", get(list_tenant_invoices))
            .route("/invoices/:id", get(get_invoice))
            .route("/invoices/:id/bolt11", get(get_invoice_bolt11))
            .route(
                "/tenants/:pubkey/stripe/session",
                get(create_stripe_session),
            )
            .route("/stripe/webhook", post(stripe_webhook))
            .with_state(state)
    }

    fn extract_auth_pubkey(&self, headers: &HeaderMap) -> std::result::Result<String, ApiError> {
        let auth = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| anyhow!("missing authorization header"))
            .map_err(ApiError::Unauthorized)?;
        if !auth.starts_with("Nostr ") {
            return Err(ApiError::Unauthorized(anyhow!(
                "authorization must use Nostr scheme"
            )));
        }

        let (_, b64) = auth
            .split_once(' ')
            .ok_or_else(|| anyhow!("malformed authorization header"))
            .map_err(ApiError::Unauthorized)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(anyhow::Error::from)
            .map_err(ApiError::Unauthorized)?;
        let json = String::from_utf8(bytes)
            .map_err(anyhow::Error::from)
            .map_err(ApiError::Unauthorized)?;
        let event = Event::from_json(json)
            .map_err(anyhow::Error::from)
            .map_err(ApiError::Unauthorized)?;

        if event.kind != Kind::HttpAuth {
            return Err(ApiError::Unauthorized(anyhow!("invalid nip98 kind")));
        }
        event
            .verify()
            .map_err(anyhow::Error::from)
            .map_err(ApiError::Unauthorized)?;

        let mut got_u = None::<String>;
        for tag in event.tags.iter() {
            let values = tag.as_slice();
            if values.len() >= 2 && values[0] == "u" {
                got_u = Some(values[1].to_string());
            }
        }

        let Some(got_u) = got_u else {
            return Err(ApiError::Unauthorized(anyhow!("missing u tag")));
        };

        if !self.host.is_empty() && !got_u.contains(&self.host) {
            return Err(ApiError::Unauthorized(anyhow!(
                "authorization host mismatch"
            )));
        }

        Ok(event.pubkey.to_hex())
    }

    fn require_admin(&self, authorized_pubkey: &str) -> std::result::Result<(), ApiError> {
        if self.admins.iter().any(|a| a == authorized_pubkey) {
            Ok(())
        } else {
            Err(ApiError::Forbidden("admin required"))
        }
    }

    fn require_admin_or_tenant(
        &self,
        authorized_pubkey: &str,
        tenant_pubkey: &str,
    ) -> std::result::Result<(), ApiError> {
        if self.admins.iter().any(|a| a == authorized_pubkey) || authorized_pubkey == tenant_pubkey
        {
            Ok(())
        } else {
            Err(ApiError::Forbidden("not authorized"))
        }
    }

    async fn get_tenant_or_404(&self, pubkey: &str) -> std::result::Result<Tenant, ApiError> {
        match self.query.get_tenant(pubkey).await {
            Ok(Some(t)) => Ok(t),
            Ok(None) => Err(ApiError::NotFound("tenant not found")),
            Err(e) => Err(ApiError::Internal(e.to_string())),
        }
    }

    fn prepare_relay(&self, mut relay: Relay) -> anyhow::Result<Relay> {
        if !relay
            .subdomain
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(anyhow!("invalid-subdomain"));
        }

        if relay.plan == "free" && relay.blossom_enabled == 1 {
            return Err(anyhow!("premium-feature"));
        }
        if relay.plan == "free" && relay.livekit_enabled == 1 {
            return Err(anyhow!("premium-feature"));
        }

        if relay.schema.is_empty() {
            relay.schema = format!("{}_{}", relay.subdomain.replace('-', "_"), relay.id);
        }
        if relay.status.is_empty() {
            relay.status = RELAY_STATUS_ACTIVE.to_string();
        }
        relay.policy_public_join = parse_bool_default(relay.policy_public_join, 0);
        relay.policy_strip_signatures = parse_bool_default(relay.policy_strip_signatures, 0);
        relay.groups_enabled = parse_bool_default(relay.groups_enabled, 1);
        relay.management_enabled = parse_bool_default(relay.management_enabled, 1);
        relay.blossom_enabled = parse_bool_default(
            relay.blossom_enabled,
            if relay.plan == "free" { 0 } else { 1 },
        );
        relay.livekit_enabled = parse_bool_default(
            relay.livekit_enabled,
            if relay.plan == "free" { 0 } else { 1 },
        );
        relay.push_enabled = parse_bool_default(relay.push_enabled, 1);

        Ok(relay)
    }
}

fn ok<T: Serialize>(status: StatusCode, data: T) -> Response {
    (status, Json(OkResponse { data, code: "ok" })).into_response()
}

fn err(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: message.to_string(),
            code: code.to_string(),
        }),
    )
        .into_response()
}

fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

fn parse_bool_default(value: i64, default: i64) -> i64 {
    if value == 0 || value == 1 {
        value
    } else {
        default
    }
}

fn map_unique_error(err: &anyhow::Error) -> Option<&'static str> {
    let sqlx_err = err.downcast_ref::<sqlx::Error>()?;
    let sqlx::Error::Database(db_err) = sqlx_err else {
        return None;
    };
    if db_err.message().contains("pubkey") {
        return Some("pubkey-exists");
    }
    if db_err.message().contains("subdomain") {
        return Some("subdomain-exists");
    }
    None
}

#[derive(Deserialize)]
struct UpdateTenantRequest {
    nwc_url: Option<String>,
}

#[derive(Serialize)]
struct IdentityResponse {
    pubkey: String,
    is_admin: bool,
}

#[derive(Deserialize)]
struct CreateRelayRequest {
    tenant: String,
    subdomain: String,
    plan: String,
    info_name: Option<String>,
    info_icon: Option<String>,
    info_description: Option<String>,
    policy_public_join: Option<i64>,
    policy_strip_signatures: Option<i64>,
    groups_enabled: Option<i64>,
    management_enabled: Option<i64>,
    blossom_enabled: Option<i64>,
    livekit_enabled: Option<i64>,
    push_enabled: Option<i64>,
}

#[derive(Deserialize)]
struct UpdateRelayRequest {
    subdomain: Option<String>,
    plan: Option<String>,
    info_name: Option<String>,
    info_icon: Option<String>,
    info_description: Option<String>,
    policy_public_join: Option<i64>,
    policy_strip_signatures: Option<i64>,
    groups_enabled: Option<i64>,
    management_enabled: Option<i64>,
    blossom_enabled: Option<i64>,
    livekit_enabled: Option<i64>,
    push_enabled: Option<i64>,
}

async fn list_tenants(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> std::result::Result<Response, ApiError> {
    let pubkey = state.api.extract_auth_pubkey(&headers)?;
    state.api.require_admin(&pubkey)?;

    match state.api.query.list_tenants().await {
        Ok(tenants) => Ok(ok(StatusCode::OK, tenants)),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

async fn list_plans() -> Response {
    ok(StatusCode::OK, Query::list_plans())
}

async fn get_identity(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> std::result::Result<Response, ApiError> {
    let pubkey = state.api.extract_auth_pubkey(&headers)?;
    let is_admin = state.api.admins.iter().any(|a| a == &pubkey);

    // Ensure tenant exists.
    match state.api.query.get_tenant(&pubkey).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            let stripe_customer_id = match state.api.billing.stripe_create_customer(&pubkey).await {
                Ok(id) => id,
                Err(e) => {
                    return Ok(err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "stripe-customer-create-failed",
                        &e.to_string(),
                    ));
                }
            };

            let tenant = Tenant {
                pubkey: pubkey.clone(),
                nwc_url: String::new(),
                nwc_error: None,
                created_at: now_ts(),
                stripe_customer_id,
                stripe_subscription_id: None,
                past_due_at: None,
            };

            match state.api.command.create_tenant(&tenant).await {
                Ok(()) => {}
                Err(e) if matches!(map_unique_error(&e), Some("pubkey-exists")) => {}
                Err(e) => {
                    return Ok(err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal",
                        &e.to_string(),
                    ));
                }
            };
        }
        Err(e) => {
            return Ok(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            ));
        }
    }

    Ok(ok(StatusCode::OK, IdentityResponse { pubkey, is_admin }))
}

async fn get_plan(Path(id): Path<String>) -> Response {
    match Query::list_plans().into_iter().find(|p| p.id == id) {
        Some(plan) => ok(StatusCode::OK, plan),
        None => err(StatusCode::NOT_FOUND, "not-found", "plan not found"),
    }
}

async fn get_tenant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pubkey): Path<String>,
) -> std::result::Result<Response, ApiError> {
    let auth = state.api.extract_auth_pubkey(&headers)?;
    state.api.require_admin_or_tenant(&auth, &pubkey)?;
    let tenant = state.api.get_tenant_or_404(&pubkey).await?;
    Ok(ok(StatusCode::OK, tenant))
}

async fn list_relays(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> std::result::Result<Response, ApiError> {
    let pubkey = state.api.extract_auth_pubkey(&headers)?;
    state.api.require_admin(&pubkey)?;

    match state.api.query.list_relays().await {
        Ok(relays) => Ok(ok(StatusCode::OK, relays)),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

async fn list_tenant_relays(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pubkey): Path<String>,
) -> std::result::Result<Response, ApiError> {
    let auth = state.api.extract_auth_pubkey(&headers)?;
    state.api.require_admin_or_tenant(&auth, &pubkey)?;

    match state.api.query.list_relays_for_tenant(&pubkey).await {
        Ok(relays) => Ok(ok(StatusCode::OK, relays)),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

async fn get_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> std::result::Result<Response, ApiError> {
    let auth = state.api.extract_auth_pubkey(&headers)?;

    let relay = match state.api.query.get_relay(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(err(StatusCode::NOT_FOUND, "not-found", "relay not found")),
        Err(e) => {
            return Ok(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            ));
        }
    };

    state.api.require_admin_or_tenant(&auth, &relay.tenant)?;

    Ok(ok(StatusCode::OK, relay))
}

async fn list_relay_activity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> std::result::Result<Response, ApiError> {
    let auth = state.api.extract_auth_pubkey(&headers)?;

    let relay = match state.api.query.get_relay(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(err(StatusCode::NOT_FOUND, "not-found", "relay not found")),
        Err(e) => {
            return Ok(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            ));
        }
    };

    state.api.require_admin_or_tenant(&auth, &relay.tenant)?;

    match state.api.query.list_activity_for_relay(&id).await {
        Ok(activity) => Ok(ok(
            StatusCode::OK,
            serde_json::json!({ "activity": activity }),
        )),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

async fn create_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateRelayRequest>,
) -> std::result::Result<Response, ApiError> {
    let auth = state.api.extract_auth_pubkey(&headers)?;
    state.api.require_admin_or_tenant(&auth, &payload.tenant)?;

    let mut relay = Relay {
        id: uuid::Uuid::new_v4().to_string(),
        tenant: payload.tenant,
        schema: String::new(),
        subdomain: payload.subdomain,
        plan: payload.plan,
        stripe_subscription_item_id: None,
        status: RELAY_STATUS_ACTIVE.to_string(),
        sync_error: String::new(),
        info_name: payload.info_name.unwrap_or_default(),
        info_icon: payload.info_icon.unwrap_or_default(),
        info_description: payload.info_description.unwrap_or_default(),
        policy_public_join: payload.policy_public_join.unwrap_or(0),
        policy_strip_signatures: payload.policy_strip_signatures.unwrap_or(0),
        groups_enabled: payload.groups_enabled.unwrap_or(1),
        management_enabled: payload.management_enabled.unwrap_or(1),
        blossom_enabled: payload.blossom_enabled.unwrap_or(0),
        livekit_enabled: payload.livekit_enabled.unwrap_or(0),
        push_enabled: payload.push_enabled.unwrap_or(1),
        synced: 0,
    };

    relay = match state.api.prepare_relay(relay) {
        Ok(r) => r,
        Err(e) if e.to_string() == "premium-feature" => {
            return Ok(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "premium-feature",
                "feature requires a paid plan",
            ));
        }
        Err(_) => {
            return Ok(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid-relay",
                "relay validation failed",
            ));
        }
    };

    match state.api.command.create_relay(&relay).await {
        Ok(()) => Ok(ok(StatusCode::CREATED, relay)),
        Err(e) => {
            if matches!(map_unique_error(&e), Some("subdomain-exists")) {
                Ok(err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "subdomain-exists",
                    "subdomain already exists",
                ))
            } else {
                Ok(err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    &e.to_string(),
                ))
            }
        }
    }
}

async fn update_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<UpdateRelayRequest>,
) -> std::result::Result<Response, ApiError> {
    let auth = state.api.extract_auth_pubkey(&headers)?;

    let mut relay = match state.api.query.get_relay(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(err(StatusCode::NOT_FOUND, "not-found", "relay not found")),
        Err(e) => {
            return Ok(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            ));
        }
    };

    state.api.require_admin_or_tenant(&auth, &relay.tenant)?;

    if let Some(v) = payload.subdomain {
        relay.subdomain = v;
    }
    if let Some(v) = payload.plan {
        relay.plan = v;
    }
    if let Some(v) = payload.info_name {
        relay.info_name = v;
    }
    if let Some(v) = payload.info_icon {
        relay.info_icon = v;
    }
    if let Some(v) = payload.info_description {
        relay.info_description = v;
    }
    if let Some(v) = payload.policy_public_join {
        relay.policy_public_join = v;
    }
    if let Some(v) = payload.policy_strip_signatures {
        relay.policy_strip_signatures = v;
    }
    if let Some(v) = payload.groups_enabled {
        relay.groups_enabled = v;
    }
    if let Some(v) = payload.management_enabled {
        relay.management_enabled = v;
    }
    if let Some(v) = payload.blossom_enabled {
        relay.blossom_enabled = v;
    }
    if let Some(v) = payload.livekit_enabled {
        relay.livekit_enabled = v;
    }
    if let Some(v) = payload.push_enabled {
        relay.push_enabled = v;
    }

    relay = match state.api.prepare_relay(relay) {
        Ok(r) => r,
        Err(e) if e.to_string() == "premium-feature" => {
            return Ok(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "premium-feature",
                "feature requires a paid plan",
            ));
        }
        Err(_) => {
            return Ok(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid-relay",
                "relay validation failed",
            ));
        }
    };

    match state.api.command.update_relay(&relay).await {
        Ok(()) => Ok(ok(StatusCode::OK, relay)),
        Err(e) => {
            if matches!(map_unique_error(&e), Some("subdomain-exists")) {
                Ok(err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "subdomain-exists",
                    "subdomain already exists",
                ))
            } else {
                Ok(err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    &e.to_string(),
                ))
            }
        }
    }
}

async fn deactivate_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> std::result::Result<Response, ApiError> {
    let auth = state.api.extract_auth_pubkey(&headers)?;

    let relay = match state.api.query.get_relay(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(err(StatusCode::NOT_FOUND, "not-found", "relay not found")),
        Err(e) => {
            return Ok(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            ));
        }
    };

    state.api.require_admin_or_tenant(&auth, &relay.tenant)?;

    if relay.status == RELAY_STATUS_INACTIVE || relay.status == RELAY_STATUS_DELINQUENT {
        return Ok(err(
            StatusCode::BAD_REQUEST,
            "relay-is-inactive",
            "relay is already inactive",
        ));
    }

    match state.api.command.deactivate_relay(&relay).await {
        Ok(()) => Ok(ok(StatusCode::OK, ())),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

async fn reactivate_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> std::result::Result<Response, ApiError> {
    let auth = state.api.extract_auth_pubkey(&headers)?;

    let relay = match state.api.query.get_relay(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(err(StatusCode::NOT_FOUND, "not-found", "relay not found")),
        Err(e) => {
            return Ok(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            ));
        }
    };

    state.api.require_admin_or_tenant(&auth, &relay.tenant)?;

    if relay.status == RELAY_STATUS_ACTIVE {
        return Ok(err(
            StatusCode::BAD_REQUEST,
            "relay-is-active",
            "relay is already active",
        ));
    }

    match state.api.command.activate_relay(&relay).await {
        Ok(()) => Ok(ok(StatusCode::OK, ())),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

async fn list_tenant_invoices(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pubkey): Path<String>,
) -> std::result::Result<Response, ApiError> {
    let auth = state.api.extract_auth_pubkey(&headers)?;
    state.api.require_admin_or_tenant(&auth, &pubkey)?;
    let tenant = state.api.get_tenant_or_404(&pubkey).await?;

    match state
        .api
        .billing
        .stripe_list_invoices(&tenant.stripe_customer_id)
        .await
    {
        Ok(invoices) => Ok(ok(StatusCode::OK, invoices)),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

async fn get_invoice(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> std::result::Result<Response, ApiError> {
    let auth = state.api.extract_auth_pubkey(&headers)?;
    let (invoice, tenant) = state
        .api
        .billing
        .get_invoice_with_tenant(&id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    state.api.require_admin_or_tenant(&auth, &tenant.pubkey)?;

    Ok(ok(StatusCode::OK, invoice))
}

async fn get_invoice_bolt11(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> std::result::Result<Response, ApiError> {
    let auth = state.api.extract_auth_pubkey(&headers)?;
    let (invoice, tenant) = state
        .api
        .billing
        .get_invoice_with_tenant(&id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    state.api.require_admin_or_tenant(&auth, &tenant.pubkey)?;

    let status = invoice["status"].as_str().unwrap_or_default();
    if status != "open" {
        return Ok(err(
            StatusCode::BAD_REQUEST,
            "invoice-not-open",
            "invoice is not open",
        ));
    }

    let amount_due = invoice["amount_due"].as_i64().unwrap_or(0);
    let currency = invoice["currency"].as_str().unwrap_or("usd");

    match state.api.billing.create_bolt11(amount_due, currency).await {
        Ok(bolt11) => Ok(ok(StatusCode::OK, serde_json::json!({ "bolt11": bolt11 }))),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

async fn create_stripe_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pubkey): Path<String>,
) -> std::result::Result<Response, ApiError> {
    let auth = state.api.extract_auth_pubkey(&headers)?;
    state.api.require_admin_or_tenant(&auth, &pubkey)?;
    let tenant = state.api.get_tenant_or_404(&pubkey).await?;

    match state
        .api
        .billing
        .stripe_create_portal_session(&tenant.stripe_customer_id)
        .await
    {
        Ok(url) => Ok(ok(StatusCode::OK, serde_json::json!({ "url": url }))),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

async fn update_tenant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pubkey): Path<String>,
    Json(payload): Json<UpdateTenantRequest>,
) -> std::result::Result<Response, ApiError> {
    let auth = state.api.extract_auth_pubkey(&headers)?;
    state.api.require_admin_or_tenant(&auth, &pubkey)?;
    let mut tenant = state.api.get_tenant_or_404(&pubkey).await?;

    if let Some(nwc_url) = payload.nwc_url {
        tenant.nwc_url = nwc_url;
    }

    match state.api.command.update_tenant(&tenant).await {
        Ok(()) => Ok(ok(StatusCode::OK, tenant)),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}
