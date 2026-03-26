use std::sync::Arc;

use anyhow::{Result, anyhow};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use base64::Engine;
use nostr_sdk::{Event, JsonUtil, Kind};
use serde::{Deserialize, Serialize};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::models::{Relay, Tenant};
use crate::repo::Repo;

#[derive(Clone)]
pub struct Api {
    host: String,
    port: u16,
    admins: Vec<String>,
    origins: Vec<String>,
    repo: Repo,
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

impl Api {
    pub fn new(repo: Repo) -> Self {
        let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = std::env::var("PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3000);
        let admins = std::env::var("ADMINS")
            .unwrap_or_default()
            .split(',')
            .map(|v| v.trim().to_lowercase())
            .filter(|v| !v.is_empty())
            .collect();
        let origins = std::env::var("ALLOW_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect();
        Self {
            host,
            port,
            admins,
            origins,
            repo,
        }
    }

    pub async fn serve(&self) -> Result<()> {
        let state = AppState {
            api: Arc::new(self.clone()),
        };

        let app = Router::new()
            .route("/plans", get(list_plans))
            .route("/plans/:id", get(get_plan))
            .route("/tenants", get(list_tenants).post(create_tenant))
            .route("/tenants/:pubkey", get(get_tenant))
            .route("/tenants/:pubkey/billing", put(update_tenant_billing))
            .route("/relays", get(list_relays).post(create_relay))
            .route("/relays/:id", get(get_relay).put(update_relay))
            .route("/relays/:id/deactivate", post(deactivate_relay))
            .route("/invoices", get(list_invoices))
            .with_state(state)
            .layer(self.cors_layer());

        let listener =
            tokio::net::TcpListener::bind(format!("{}:{}", self.host, self.port)).await?;
        axum::serve(listener, app).await?;
        Ok(())
    }

    fn cors_layer(&self) -> CorsLayer {
        if self.origins.is_empty() {
            CorsLayer::permissive()
        } else {
            let origins = self
                .origins
                .iter()
                .filter_map(|o| o.parse::<axum::http::HeaderValue>().ok())
                .collect::<Vec<_>>();
            CorsLayer::new().allow_origin(AllowOrigin::list(origins))
        }
    }

    fn is_admin(&self, pubkey: &str) -> bool {
        self.admins.iter().any(|a| a == pubkey)
    }

    fn is_tenant(&self, authorized_pubkey: &str, tenant_pubkey: &str) -> bool {
        authorized_pubkey == tenant_pubkey
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

fn prepare_relay(mut relay: Relay) -> anyhow::Result<Relay> {
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
        relay.status = "new".to_string();
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

fn auth_fail_response(e: anyhow::Error) -> Response {
    err(StatusCode::UNAUTHORIZED, "unauthorized", &e.to_string())
}

fn extract_auth_pubkey(
    headers: &HeaderMap,
    method: &Method,
    _uri: &Uri,
    host: &str,
) -> Result<String> {
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow!("missing authorization header"))?;
    if !auth.starts_with("Nostr ") {
        return Err(anyhow!("authorization must use Nostr scheme"));
    }

    let (_, b64) = auth
        .split_once(' ')
        .ok_or_else(|| anyhow!("malformed authorization header"))?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64)?;
    let json = String::from_utf8(bytes)?;
    let event = Event::from_json(json)?;

    if event.kind != Kind::HttpAuth {
        return Err(anyhow!("invalid nip98 kind"));
    }
    event.verify()?;

    let expected_host = host;
    let want_m = method.as_str();

    let mut got_u = None::<String>;
    let mut got_m = None::<String>;
    for tag in event.tags.iter() {
        let values = tag.as_slice();
        if values.len() >= 2 {
            if values[0] == "u" {
                got_u = Some(values[1].to_string());
            } else if values[0] == "method" {
                got_m = Some(values[1].to_string());
            }
        }
    }

    let Some(got_u) = got_u else {
        return Err(anyhow!("missing u tag"));
    };
    let Some(got_m) = got_m else {
        return Err(anyhow!("missing method tag"));
    };

    if !expected_host.is_empty() && !got_u.contains(expected_host) {
        return Err(anyhow!("authorization host mismatch"));
    }
    if got_m != want_m {
        return Err(anyhow!("authorization method mismatch"));
    }

    Ok(event.pubkey.to_hex())
}

#[derive(Deserialize)]
struct TenantParam {
    tenant: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct UpdateTenantBillingRequest {
    nwc_url: String,
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
    method: Method,
    uri: Uri,
) -> Response {
    let pubkey = match extract_auth_pubkey(&headers, &method, &uri, &state.api.host) {
        Ok(v) => v,
        Err(e) => return auth_fail_response(e),
    };
    if !state.api.is_admin(&pubkey) {
        return err(StatusCode::FORBIDDEN, "forbidden", "admin required");
    }
    match state.api.repo.list_tenants().await {
        Ok(tenants) => ok(StatusCode::OK, tenants),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        ),
    }
}

async fn list_plans(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
) -> Response {
    if let Err(e) = extract_auth_pubkey(&headers, &method, &uri, &state.api.host) {
        return auth_fail_response(e);
    }

    ok(StatusCode::OK, Repo::list_plans())
}

async fn get_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(id): Path<String>,
) -> Response {
    if let Err(e) = extract_auth_pubkey(&headers, &method, &uri, &state.api.host) {
        return auth_fail_response(e);
    }

    match Repo::list_plans().into_iter().find(|p| p.id == id) {
        Some(plan) => ok(StatusCode::OK, plan),
        None => err(StatusCode::NOT_FOUND, "not-found", "plan not found"),
    }
}

async fn get_tenant(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(pubkey): Path<String>,
) -> Response {
    let auth = match extract_auth_pubkey(&headers, &method, &uri, &state.api.host) {
        Ok(v) => v,
        Err(e) => return auth_fail_response(e),
    };
    if !(state.api.is_admin(&auth) || state.api.is_tenant(&auth, &pubkey)) {
        return err(StatusCode::FORBIDDEN, "forbidden", "not authorized");
    }
    match state.api.repo.get_tenant(&pubkey).await {
        Ok(Some(tenant)) => ok(StatusCode::OK, tenant),
        Ok(None) => err(StatusCode::NOT_FOUND, "not-found", "tenant not found"),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        ),
    }
}

async fn create_tenant(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
) -> Response {
    let pubkey = match extract_auth_pubkey(&headers, &method, &uri, &state.api.host) {
        Ok(v) => v,
        Err(e) => return auth_fail_response(e),
    };

    let tenant = Tenant {
        pubkey: pubkey.clone(),
        nwc_url: String::new(),
        created_at: now_ts(),
        billing_anchor: now_ts(),
    };

    match state.api.repo.create_tenant(&tenant).await {
        Ok(()) => ok(StatusCode::CREATED, tenant),
        Err(e) => {
            if matches!(map_unique_error(&e), Some("pubkey-exists")) {
                err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "pubkey-exists",
                    "tenant already exists",
                )
            } else {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    &e.to_string(),
                )
            }
        }
    }
}

async fn list_relays(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Query(query): Query<TenantParam>,
) -> Response {
    let auth = match extract_auth_pubkey(&headers, &method, &uri, &state.api.host) {
        Ok(v) => v,
        Err(e) => return auth_fail_response(e),
    };

    let tenant_filter = if state.api.is_admin(&auth) {
        query.tenant.as_deref()
    } else {
        if state
            .api
            .repo
            .get_tenant(&auth)
            .await
            .ok()
            .flatten()
            .is_none()
        {
            return err(StatusCode::FORBIDDEN, "forbidden", "tenant required");
        }
        if query.tenant.is_some() {
            return err(
                StatusCode::BAD_REQUEST,
                "tenant-query-not-allowed",
                "tenant query is not allowed for tenant users",
            );
        }
        Some(auth.as_str())
    };

    match state.api.repo.list_relays(tenant_filter).await {
        Ok(relays) => ok(StatusCode::OK, relays),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        ),
    }
}

async fn get_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(id): Path<String>,
) -> Response {
    let auth = match extract_auth_pubkey(&headers, &method, &uri, &state.api.host) {
        Ok(v) => v,
        Err(e) => return auth_fail_response(e),
    };

    let relay = match state.api.repo.get_relay(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return err(StatusCode::NOT_FOUND, "not-found", "relay not found"),
        Err(e) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            );
        }
    };

    if !(state.api.is_admin(&auth) || state.api.is_tenant(&auth, &relay.tenant)) {
        return err(StatusCode::FORBIDDEN, "forbidden", "not authorized");
    }

    ok(StatusCode::OK, relay)
}

async fn create_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Json(payload): Json<CreateRelayRequest>,
) -> Response {
    let auth = match extract_auth_pubkey(&headers, &method, &uri, &state.api.host) {
        Ok(v) => v,
        Err(e) => return auth_fail_response(e),
    };

    if !(state.api.is_admin(&auth) || state.api.is_tenant(&auth, &payload.tenant)) {
        return err(StatusCode::FORBIDDEN, "forbidden", "not authorized");
    }

    let mut relay = Relay {
        id: uuid::Uuid::new_v4().to_string(),
        tenant: payload.tenant,
        schema: String::new(),
        subdomain: payload.subdomain,
        plan: payload.plan,
        status: "new".to_string(),
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
    };

    relay = match prepare_relay(relay) {
        Ok(r) => r,
        Err(e) if e.to_string() == "premium-feature" => {
            return err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "premium-feature",
                "feature requires a paid plan",
            );
        }
        Err(_) => {
            return err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid-relay",
                "relay validation failed",
            );
        }
    };

    match state.api.repo.create_relay(&relay).await {
        Ok(()) => ok(StatusCode::CREATED, relay),
        Err(e) => {
            if matches!(map_unique_error(&e), Some("subdomain-exists")) {
                err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "subdomain-exists",
                    "subdomain already exists",
                )
            } else {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    &e.to_string(),
                )
            }
        }
    }
}

async fn update_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(id): Path<String>,
    Json(payload): Json<UpdateRelayRequest>,
) -> Response {
    let auth = match extract_auth_pubkey(&headers, &method, &uri, &state.api.host) {
        Ok(v) => v,
        Err(e) => return auth_fail_response(e),
    };

    let mut relay = match state.api.repo.get_relay(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return err(StatusCode::NOT_FOUND, "not-found", "relay not found"),
        Err(e) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            );
        }
    };

    if !(state.api.is_admin(&auth) || state.api.is_tenant(&auth, &relay.tenant)) {
        return err(StatusCode::FORBIDDEN, "forbidden", "not authorized");
    }

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

    relay = match prepare_relay(relay) {
        Ok(r) => r,
        Err(e) if e.to_string() == "premium-feature" => {
            return err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "premium-feature",
                "feature requires a paid plan",
            );
        }
        Err(_) => {
            return err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid-relay",
                "relay validation failed",
            );
        }
    };

    match state.api.repo.update_relay(&relay).await {
        Ok(()) => ok(StatusCode::OK, relay),
        Err(e) => {
            if matches!(map_unique_error(&e), Some("subdomain-exists")) {
                err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "subdomain-exists",
                    "subdomain already exists",
                )
            } else {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    &e.to_string(),
                )
            }
        }
    }
}

async fn deactivate_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(id): Path<String>,
) -> Response {
    let auth = match extract_auth_pubkey(&headers, &method, &uri, &state.api.host) {
        Ok(v) => v,
        Err(e) => return auth_fail_response(e),
    };

    let relay = match state.api.repo.get_relay(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return err(StatusCode::NOT_FOUND, "not-found", "relay not found"),
        Err(e) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            );
        }
    };

    if !(state.api.is_admin(&auth) || state.api.is_tenant(&auth, &relay.tenant)) {
        return err(StatusCode::FORBIDDEN, "forbidden", "not authorized");
    }

    match state.api.repo.deactivate_relay(&relay).await {
        Ok(()) => ok(StatusCode::OK, ()),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        ),
    }
}

async fn list_invoices(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Query(query): Query<TenantParam>,
) -> Response {
    let auth = match extract_auth_pubkey(&headers, &method, &uri, &state.api.host) {
        Ok(v) => v,
        Err(e) => return auth_fail_response(e),
    };

    let tenant_filter = if state.api.is_admin(&auth) {
        query.tenant.as_deref()
    } else {
        if state
            .api
            .repo
            .get_tenant(&auth)
            .await
            .ok()
            .flatten()
            .is_none()
        {
            return err(StatusCode::FORBIDDEN, "forbidden", "tenant required");
        }
        if query.tenant.is_some() {
            return err(
                StatusCode::BAD_REQUEST,
                "tenant-query-not-allowed",
                "tenant query is not allowed for tenant users",
            );
        }
        Some(auth.as_str())
    };

    match state.api.repo.list_invoices(tenant_filter).await {
        Ok(invoices) => ok(StatusCode::OK, invoices),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        ),
    }
}

async fn update_tenant_billing(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(pubkey): Path<String>,
    Json(payload): Json<UpdateTenantBillingRequest>,
) -> Response {
    let auth = match extract_auth_pubkey(&headers, &method, &uri, &state.api.host) {
        Ok(v) => v,
        Err(e) => return auth_fail_response(e),
    };

    if !(state.api.is_admin(&auth) || state.api.is_tenant(&auth, &pubkey)) {
        return err(StatusCode::FORBIDDEN, "forbidden", "not authorized");
    }

    match state
        .api
        .repo
        .update_tenant_nwc_url(&pubkey, &payload.nwc_url)
        .await
    {
        Ok(()) => ok(StatusCode::OK, payload),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        ),
    }
}
