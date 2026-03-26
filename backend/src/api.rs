use std::sync::Arc;

use anyhow::{Result, anyhow};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
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

#[derive(Debug)]
enum ApiError {
    Unauthorized(anyhow::Error),
    Forbidden(&'static str),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthorized(e) => err(StatusCode::UNAUTHORIZED, "unauthorized", &e.to_string()),
            Self::Forbidden(message) => err(StatusCode::FORBIDDEN, "forbidden", message),
        }
    }
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
        let app = self.router();

        let listener =
            tokio::net::TcpListener::bind(format!("{}:{}", self.host, self.port)).await?;
        axum::serve(listener, app).await?;
        Ok(())
    }

    fn router(&self) -> Router {
        let state = AppState {
            api: Arc::new(self.clone()),
        };

        Router::new()
            .route("/plans", get(list_plans))
            .route("/plans/:id", get(get_plan))
            .route("/tenants", get(list_tenants).post(create_tenant))
            .route("/tenants/:pubkey", get(get_tenant))
            .route("/tenants/:pubkey/relays", get(list_tenant_relays))
            .route("/tenants/:pubkey/invoices", get(list_tenant_invoices))
            .route("/tenants/:pubkey/billing", put(update_tenant_billing))
            .route("/relays", get(list_relays).post(create_relay))
            .route("/relays/:id", get(get_relay).put(update_relay))
            .route("/relays/:id/deactivate", post(deactivate_relay))
            .route("/invoices", get(list_invoices))
            .route("/invoices/:id", get(get_invoice))
            .with_state(state)
            .layer(self.cors_layer())
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
) -> std::result::Result<Response, ApiError> {
    let pubkey = state.api.extract_auth_pubkey(&headers)?;
    state.api.require_admin(&pubkey)?;

    match state.api.repo.list_tenants().await {
        Ok(tenants) => Ok(ok(StatusCode::OK, tenants)),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

async fn list_plans(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> std::result::Result<Response, ApiError> {
    let _ = state.api.extract_auth_pubkey(&headers)?;

    Ok(ok(StatusCode::OK, Repo::list_plans()))
}

async fn get_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> std::result::Result<Response, ApiError> {
    let _ = state.api.extract_auth_pubkey(&headers)?;

    match Repo::list_plans().into_iter().find(|p| p.id == id) {
        Some(plan) => Ok(ok(StatusCode::OK, plan)),
        None => Ok(err(StatusCode::NOT_FOUND, "not-found", "plan not found")),
    }
}

async fn get_tenant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pubkey): Path<String>,
) -> std::result::Result<Response, ApiError> {
    let auth = state.api.extract_auth_pubkey(&headers)?;
    state.api.require_admin_or_tenant(&auth, &pubkey)?;

    match state.api.repo.get_tenant(&pubkey).await {
        Ok(Some(tenant)) => Ok(ok(StatusCode::OK, tenant)),
        Ok(None) => Ok(err(StatusCode::NOT_FOUND, "not-found", "tenant not found")),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

async fn create_tenant(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> std::result::Result<Response, ApiError> {
    let pubkey = state.api.extract_auth_pubkey(&headers)?;

    let tenant = Tenant {
        pubkey: pubkey.clone(),
        nwc_url: String::new(),
        created_at: now_ts(),
        billing_anchor: now_ts(),
    };

    match state.api.repo.create_tenant(&tenant).await {
        Ok(()) => Ok(ok(StatusCode::CREATED, tenant)),
        Err(e) => {
            if matches!(map_unique_error(&e), Some("pubkey-exists")) {
                Ok(err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "pubkey-exists",
                    "tenant already exists",
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

async fn list_relays(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> std::result::Result<Response, ApiError> {
    let pubkey = state.api.extract_auth_pubkey(&headers)?;
    state.api.require_admin(&pubkey)?;

    match state.api.repo.list_relays().await {
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

    match state.api.repo.list_relays_for_tenant(&pubkey).await {
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

    let relay = match state.api.repo.get_relay(&id).await {
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

    match state.api.repo.create_relay(&relay).await {
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

    let mut relay = match state.api.repo.get_relay(&id).await {
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

    match state.api.repo.update_relay(&relay).await {
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

    let relay = match state.api.repo.get_relay(&id).await {
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

    match state.api.repo.deactivate_relay(&relay).await {
        Ok(()) => Ok(ok(StatusCode::OK, ())),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

async fn list_invoices(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> std::result::Result<Response, ApiError> {
    let pubkey = state.api.extract_auth_pubkey(&headers)?;
    state.api.require_admin(&pubkey)?;

    match state.api.repo.list_invoices().await {
        Ok(invoices) => Ok(ok(StatusCode::OK, invoices)),
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

    match state.api.repo.list_invoices_for_tenant(&pubkey).await {
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

    let invoice = match state.api.repo.get_invoice(&id).await {
        Ok(Some(i)) => i,
        Ok(None) => return Ok(err(StatusCode::NOT_FOUND, "not-found", "invoice not found")),
        Err(e) => {
            return Ok(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            ));
        }
    };

    state.api.require_admin_or_tenant(&auth, &invoice.tenant)?;

    Ok(ok(StatusCode::OK, invoice))
}

async fn update_tenant_billing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pubkey): Path<String>,
    Json(payload): Json<UpdateTenantBillingRequest>,
) -> std::result::Result<Response, ApiError> {
    let auth = state.api.extract_auth_pubkey(&headers)?;
    state.api.require_admin_or_tenant(&auth, &pubkey)?;

    match state
        .api
        .repo
        .update_tenant_nwc_url(&pubkey, &payload.nwc_url)
        .await
    {
        Ok(()) => Ok(ok(StatusCode::OK, payload)),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use base64::Engine;
    use nostr_sdk::{EventBuilder, JsonUtil, Keys, Kind, Tag};
    use serde_json::{Value, json};
    use sqlx::{SqlitePool, sqlite::{SqliteConnectOptions, SqlitePoolOptions}};
    use tower::util::ServiceExt;

    use crate::{models::{Relay, Tenant}, repo::Repo};

    use super::Api;

    async fn test_repo() -> Repo {
        let db_file = std::env::temp_dir().join(format!("caravel-api-test-{}.db", uuid::Uuid::new_v4()));
        let database_url = format!("sqlite://{}", db_file.display());

        let options = SqliteConnectOptions::from_str(&database_url)
            .expect("sqlite options")
            .create_if_missing(true);
        let pool: SqlitePool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("connect sqlite");

        sqlx::query("PRAGMA journal_mode = WAL;")
            .execute(&pool)
            .await
            .expect("set WAL");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");

        Repo { pool }
    }

    fn keys() -> (Keys, Keys, Keys) {
        (Keys::generate(), Keys::generate(), Keys::generate())
    }

    fn pubkey_hex(keys: &Keys) -> String {
        keys.public_key().to_hex()
    }

    fn auth_header(keys: &Keys, u: &str) -> String {
        let tag = Tag::parse(["u", u]).expect("u tag");
        let event = EventBuilder::new(Kind::HttpAuth, "").tags([tag])
            .sign_with_keys(keys)
            .expect("sign nip98 event");
        let json = event.as_json();
        let b64 = base64::engine::general_purpose::STANDARD.encode(json);
        format!("Nostr {b64}")
    }

    fn make_api(repo: Repo, admin_pubkey: String) -> Api {
        Api {
            host: "api.test".to_string(),
            port: 0,
            admins: vec![admin_pubkey],
            origins: vec![],
            repo,
        }
    }

    async fn request(
        api: &Api,
        method: &str,
        path: &str,
        auth: Option<String>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(auth) = auth {
            builder = builder.header(header::AUTHORIZATION, auth);
        }

        let req = if let Some(body) = body {
            builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("request")
        } else {
            builder.body(Body::empty()).expect("request")
        };

        let resp = api.router().oneshot(req).await.expect("router response");
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("read body");
        let payload: Value = serde_json::from_slice(&bytes).expect("json body");
        (status, payload)
    }

    async fn create_tenant(repo: &Repo, pubkey: String) {
        let tenant = Tenant {
            pubkey,
            nwc_url: String::new(),
            created_at: 1,
            billing_anchor: 1,
        };
        repo.create_tenant(&tenant).await.expect("create tenant");
    }

    async fn create_relay(repo: &Repo, id: &str, tenant: &str, subdomain: &str) {
        let relay = Relay {
            id: id.to_string(),
            tenant: tenant.to_string(),
            schema: format!("{}_{}", subdomain.replace('-', "_"), id),
            subdomain: subdomain.to_string(),
            plan: "free".to_string(),
            status: "new".to_string(),
            sync_error: String::new(),
            info_name: String::new(),
            info_icon: String::new(),
            info_description: String::new(),
            policy_public_join: 0,
            policy_strip_signatures: 0,
            groups_enabled: 1,
            management_enabled: 1,
            blossom_enabled: 0,
            livekit_enabled: 0,
            push_enabled: 1,
        };
        repo.create_relay(&relay).await.expect("create relay");
    }

    #[tokio::test]
    async fn missing_auth_returns_unauthorized() {
        let repo = test_repo().await;
        let (admin_keys, _, _) = keys();
        let api = make_api(repo, pubkey_hex(&admin_keys));

        let (status, body) = request(&api, "GET", "/plans", None, None).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["code"], "unauthorized");
    }

    #[tokio::test]
    async fn plans_endpoints_return_ok_and_not_found() {
        let repo = test_repo().await;
        let (admin_keys, _, _) = keys();
        let api = make_api(repo, pubkey_hex(&admin_keys));
        let auth = auth_header(&admin_keys, "https://api.test");

        let (status, body) = request(&api, "GET", "/plans", Some(auth.clone()), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["code"], "ok");
        assert!(body["data"].as_array().expect("plans array").len() >= 3);

        let (status, body) = request(&api, "GET", "/plans/does-not-exist", Some(auth), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["code"], "not-found");
    }

    #[tokio::test]
    async fn tenants_list_is_admin_only() {
        let repo = test_repo().await;
        let (admin_keys, tenant_keys, _) = keys();
        let api = make_api(repo, pubkey_hex(&admin_keys));

        let auth = auth_header(&tenant_keys, "https://api.test");
        let (status, body) = request(&api, "GET", "/tenants", Some(auth), None).await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["code"], "forbidden");
    }

    #[tokio::test]
    async fn create_tenant_and_duplicate_returns_expected_codes() {
        let repo = test_repo().await;
        let (admin_keys, tenant_keys, _) = keys();
        let api = make_api(repo, pubkey_hex(&admin_keys));
        let auth = auth_header(&tenant_keys, "https://api.test");

        let (status, body) = request(&api, "POST", "/tenants", Some(auth.clone()), None).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["code"], "ok");
        assert_eq!(body["data"]["pubkey"], pubkey_hex(&tenant_keys));

        let (status, body) = request(&api, "POST", "/tenants", Some(auth), None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["code"], "pubkey-exists");
    }

    #[tokio::test]
    async fn tenant_get_allows_owner_and_denies_other_tenant() {
        let repo = test_repo().await;
        let (admin_keys, tenant_a_keys, tenant_b_keys) = keys();
        create_tenant(&repo, pubkey_hex(&tenant_a_keys)).await;
        let api = make_api(repo, pubkey_hex(&admin_keys));

        let owner_auth = auth_header(&tenant_a_keys, "https://api.test");
        let (status, body) = request(
            &api,
            "GET",
            &format!("/tenants/{}", pubkey_hex(&tenant_a_keys)),
            Some(owner_auth),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["code"], "ok");

        let other_auth = auth_header(&tenant_b_keys, "https://api.test");
        let (status, body) = request(
            &api,
            "GET",
            &format!("/tenants/{}", pubkey_hex(&tenant_a_keys)),
            Some(other_auth),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["code"], "forbidden");
    }

    #[tokio::test]
    async fn create_relay_validates_premium_feature_and_subdomain() {
        let repo = test_repo().await;
        let (admin_keys, tenant_keys, _) = keys();
        create_tenant(&repo, pubkey_hex(&tenant_keys)).await;
        let api = make_api(repo, pubkey_hex(&admin_keys));
        let auth = auth_header(&tenant_keys, "https://api.test");

        let (status, body) = request(
            &api,
            "POST",
            "/relays",
            Some(auth.clone()),
            Some(json!({
                "tenant": pubkey_hex(&tenant_keys),
                "subdomain": "bad_subdomain",
                "plan": "free"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["code"], "invalid-relay");

        let (status, body) = request(
            &api,
            "POST",
            "/relays",
            Some(auth),
            Some(json!({
                "tenant": pubkey_hex(&tenant_keys),
                "subdomain": "good-subdomain",
                "plan": "free",
                "blossom_enabled": 1
            })),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["code"], "premium-feature");
    }

    #[tokio::test]
    async fn relay_get_honors_owner_and_duplicate_subdomain_is_422() {
        let repo = test_repo().await;
        let (admin_keys, tenant_a_keys, tenant_b_keys) = keys();
        let tenant_a = pubkey_hex(&tenant_a_keys);
        create_tenant(&repo, tenant_a.clone()).await;
        create_tenant(&repo, pubkey_hex(&tenant_b_keys)).await;
        create_relay(&repo, "relay-a", &tenant_a, "alpha").await;

        let api = make_api(repo.clone(), pubkey_hex(&admin_keys));
        let owner_auth = auth_header(&tenant_a_keys, "https://api.test");

        let (status, body) = request(&api, "GET", "/relays/relay-a", Some(owner_auth), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["code"], "ok");

        let other_auth = auth_header(&tenant_b_keys, "https://api.test");
        let (status, body) = request(&api, "GET", "/relays/relay-a", Some(other_auth), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["code"], "forbidden");

        let (status, body) = request(
            &api,
            "POST",
            "/relays",
            Some(auth_header(&tenant_a_keys, "https://api.test")),
            Some(json!({
                "tenant": tenant_a,
                "subdomain": "alpha",
                "plan": "free"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["code"], "subdomain-exists");
    }

    #[tokio::test]
    async fn update_tenant_billing_allows_self() {
        let repo = test_repo().await;
        let (admin_keys, tenant_keys, _) = keys();
        let tenant = pubkey_hex(&tenant_keys);
        create_tenant(&repo, tenant.clone()).await;

        let api = make_api(repo.clone(), pubkey_hex(&admin_keys));
        let auth = auth_header(&tenant_keys, "https://api.test");

        let (status, body) = request(
            &api,
            "PUT",
            &format!("/tenants/{tenant}/billing"),
            Some(auth),
            Some(json!({"nwc_url":"nostr+walletconnect://example"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["code"], "ok");
        assert_eq!(body["data"]["nwc_url"], "nostr+walletconnect://example");
    }
}
