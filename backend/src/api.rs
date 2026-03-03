use std::sync::Arc;

use anyhow::{Result, anyhow};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};

use crate::auth::verify_nip98;
use crate::models::{NewTenant, Relay, RelayConfig};
use crate::provisioning::Provisioner;
use crate::repo::Repo;

#[derive(Clone)]
pub struct AppState {
    pub repo: Repo,
    pub admin_pubkeys: Arc<Vec<String>>,
    pub provisioner: Provisioner,
}

pub fn router(state: AppState) -> Router {
    let tenant_routes = Router::new()
        .route("/tenant", get(get_tenant))
        .route(
            "/tenant/relays",
            get(list_tenant_relays).post(create_tenant_relay),
        )
        .route(
            "/tenant/relays/:id",
            get(get_tenant_relay).put(update_tenant_relay),
        )
        .route(
            "/tenant/relays/:id/deactivate",
            post(deactivate_tenant_relay),
        )
        .route("/tenant/invoices", get(list_tenant_invoices))
        .route("/tenant/billing", put(update_tenant_billing));

    let admin_routes = Router::new()
        .route("/admin/check", get(admin_check))
        .route("/admin/tenants", get(admin_list_tenants))
        .route(
            "/admin/tenants/:pubkey",
            get(admin_get_tenant).put(admin_update_tenant_status),
        )
        .route("/admin/relays", get(admin_list_relays))
        .route(
            "/admin/relays/:id",
            get(admin_get_relay).put(admin_update_relay),
        )
        .route("/admin/relays/:id/deactivate", post(admin_deactivate_relay));

    Router::new()
        .merge(tenant_routes)
        .merge(admin_routes)
        .with_state(state)
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(self)).into_response()
    }
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(ApiError {
            error: "unauthorized".into(),
        }),
    )
        .into_response()
}

fn forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(ApiError {
            error: "forbidden".into(),
        }),
    )
        .into_response()
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: "not found".into(),
        }),
    )
        .into_response()
}

fn is_unique_subdomain_violation(err: &anyhow::Error) -> bool {
    let Some(sqlx_err) = err.downcast_ref::<sqlx::Error>() else {
        return false;
    };

    let sqlx::Error::Database(db_err) = sqlx_err else {
        return false;
    };

    db_err.message().contains("relays.subdomain")
        || db_err.message().contains("relays_subdomain_unique")
}

fn extract_auth_pubkey(headers: &HeaderMap, method: &Method, uri: &Uri) -> Result<String> {
    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow!("missing authorization header"))?;

    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");

    let path = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(uri.path());
    let url = format!("{}://{}{}", scheme, host, path);
    let pubkey = verify_nip98(auth_header, &url, method.as_str())?;
    Ok(pubkey.to_hex())
}

async fn get_tenant(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
) -> Response {
    let pubkey = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    match state.repo.get_tenant(&pubkey).await {
        Ok(Some(tenant)) => (StatusCode::OK, Json(tenant)).into_response(),
        Ok(None) => {
            let tenant = NewTenant {
                pubkey: pubkey.clone(),
                status: "active".to_string(),
                tenant_nwc_url: "".to_string(),
            };
            if state.repo.create_tenant(&tenant).await.is_ok() {
                (StatusCode::OK, Json(tenant)).into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError {
                        error: "failed to create tenant".into(),
                    }),
                )
                    .into_response()
            }
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "failed to load tenant".into(),
            }),
        )
            .into_response(),
    }
}

async fn list_tenant_relays(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
) -> Response {
    let pubkey = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    match state.repo.list_relays_by_tenant(&pubkey).await {
        Ok(relays) => (StatusCode::OK, Json(relays)).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "failed to load relays".into(),
            }),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct CreateRelayRequest {
    name: String,
    subdomain: String,
    icon: String,
    description: String,
    plan: String,
    config: Option<RelayConfig>,
}

async fn create_tenant_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Json(payload): Json<CreateRelayRequest>,
) -> Response {
    let pubkey = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    let tenant = NewTenant {
        pubkey: pubkey.clone(),
        status: "active".to_string(),
        tenant_nwc_url: "".to_string(),
    };

    if let Err(_) = state.repo.create_tenant_if_missing(&tenant).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "failed to ensure tenant".into(),
            }),
        )
            .into_response();
    }

    let id = payload.subdomain.replace('-', "_");
    let relay = Relay {
        id: id.clone(),
        tenant: pubkey.clone(),
        name: payload.name,
        subdomain: payload.subdomain.clone(),
        icon: payload.icon,
        description: payload.description,
        plan: payload.plan,
        status: "pending".to_string(),
        config: payload.config,
    };

    if let Err(err) = state.repo.upsert_relay(&relay).await {
        if is_unique_subdomain_violation(&err) {
            return (
                StatusCode::CONFLICT,
                Json(ApiError {
                    error: "subdomain already exists".into(),
                }),
            )
                .into_response();
        }

        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "failed to create relay".into(),
            }),
        )
            .into_response();
    }

    if let Err(err) = state.provisioner.create_relay(&relay).await {
        tracing::error!(relay_id = relay.id, error = %err, "zooid create failed");
        let _ = state.repo.update_relay_status(&relay.id, "provisioning_failed").await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError { error: format!("failed to provision relay: {err}") }),
        )
            .into_response();
    }

    let _ = state.repo.update_relay_status(&relay.id, "active").await;

    (StatusCode::CREATED, Json(relay)).into_response()
}

async fn get_tenant_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(id): Path<String>,
) -> Response {
    let pubkey = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    match state.repo.get_relay(&id).await {
        Ok(Some(relay)) if relay.tenant == pubkey => (StatusCode::OK, Json(relay)).into_response(),
        Ok(Some(_)) => forbidden(),
        Ok(None) => not_found(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "failed to load relay".into(),
            }),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct UpdateRelayRequest {
    name: String,
    subdomain: String,
    icon: String,
    description: String,
    plan: String,
    config: Option<RelayConfig>,
}

async fn update_tenant_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(id): Path<String>,
    Json(payload): Json<UpdateRelayRequest>,
) -> Response {
    let pubkey = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    let existing = match state.repo.get_relay(&id).await {
        Ok(Some(relay)) => relay,
        Ok(None) => return not_found(),
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "failed to load relay".into(),
                }),
            )
                .into_response();
        }
    };

    if existing.tenant != pubkey {
        return forbidden();
    }

    let relay = Relay {
        id: existing.id,
        tenant: existing.tenant,
        name: payload.name,
        subdomain: payload.subdomain.clone(),
        icon: payload.icon,
        description: payload.description,
        plan: payload.plan,
        status: existing.status,
        config: payload.config,
    };

    if let Err(err) = state.repo.upsert_relay(&relay).await {
        if is_unique_subdomain_violation(&err) {
            return (
                StatusCode::CONFLICT,
                Json(ApiError {
                    error: "subdomain already exists".into(),
                }),
            )
                .into_response();
        }

        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "failed to update relay".into(),
            }),
        )
            .into_response();
    }

    if let Err(err) = state.provisioner.update_relay(&relay).await {
        tracing::error!(relay_id = relay.id, error = %err, "zooid patch failed");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError { error: format!("failed to provision relay: {err}") }),
        )
            .into_response();
    }

    let _ = state.repo.update_relay_status(&relay.id, "active").await;

    (StatusCode::OK, Json(relay)).into_response()
}

async fn deactivate_tenant_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(id): Path<String>,
) -> Response {
    let pubkey = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    let existing = match state.repo.get_relay(&id).await {
        Ok(Some(relay)) => relay,
        Ok(None) => return not_found(),
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "failed to load relay".into(),
                }),
            )
                .into_response();
        }
    };

    if existing.tenant != pubkey {
        return forbidden();
    }

    let relay = Relay {
        status: "deactivated".to_string(),
        config: None,
        ..existing
    };

    if let Err(_) = state.repo.upsert_relay(&relay).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "failed to deactivate relay".into(),
            }),
        )
            .into_response();
    }

    (StatusCode::OK, Json(relay)).into_response()
}

async fn list_tenant_invoices(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
) -> Response {
    let pubkey = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    match state.repo.list_invoices_by_tenant(&pubkey).await {
        Ok(invoices) => (StatusCode::OK, Json(invoices)).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "failed to load invoices".into(),
            }),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct UpdateTenantBillingRequest {
    tenant_nwc_url: String,
}

async fn update_tenant_billing(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Json(payload): Json<UpdateTenantBillingRequest>,
) -> Response {
    let pubkey = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    if let Err(_) = state
        .repo
        .update_tenant_nwc_url(&pubkey, &payload.tenant_nwc_url)
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "failed to update billing".into(),
            }),
        )
            .into_response();
    }

    (StatusCode::OK, Json(payload)).into_response()
}

async fn admin_list_tenants(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
) -> Response {
    let pubkey = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    if !state.admin_pubkeys.contains(&pubkey) {
        return forbidden();
    }

    match state.repo.list_tenants().await {
        Ok(tenants) => (StatusCode::OK, Json(tenants)).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "failed to load tenants".into(),
            }),
        )
            .into_response(),
    }
}

#[derive(Debug, Serialize)]
struct AdminCheckResponse {
    is_admin: bool,
}

async fn admin_check(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
) -> Response {
    let pubkey = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    let is_admin = state.admin_pubkeys.contains(&pubkey);
    (StatusCode::OK, Json(AdminCheckResponse { is_admin })).into_response()
}

async fn admin_get_tenant(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(pubkey): Path<String>,
) -> Response {
    let admin = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    if !state.admin_pubkeys.contains(&admin) {
        return forbidden();
    }

    let tenant = match state.repo.get_tenant(&pubkey).await {
        Ok(Some(tenant)) => tenant,
        Ok(None) => return not_found(),
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "failed to load tenant".into(),
                }),
            )
                .into_response();
        }
    };

    let relays = match state.repo.list_relays_by_tenant(&pubkey).await {
        Ok(relays) => relays,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "failed to load relays".into(),
                }),
            )
                .into_response();
        }
    };

    #[derive(Serialize)]
    struct TenantDetail {
        tenant: crate::models::Tenant,
        relays: Vec<Relay>,
    }

    (StatusCode::OK, Json(TenantDetail { tenant, relays })).into_response()
}

#[derive(Debug, Deserialize)]
struct UpdateTenantStatusRequest {
    status: String,
}

async fn admin_update_tenant_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(pubkey): Path<String>,
    Json(payload): Json<UpdateTenantStatusRequest>,
) -> Response {
    let admin = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    if !state.admin_pubkeys.contains(&admin) {
        return forbidden();
    }

    let tenant = match state.repo.get_tenant(&pubkey).await {
        Ok(Some(tenant)) => tenant,
        Ok(None) => return not_found(),
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "failed to load tenant".into(),
                }),
            )
                .into_response();
        }
    };

    if let Err(_) = state
        .repo
        .update_tenant_status(&tenant.pubkey, &payload.status)
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "failed to update tenant".into(),
            }),
        )
            .into_response();
    }

    let updated = NewTenant {
        pubkey: tenant.pubkey,
        status: payload.status,
        tenant_nwc_url: tenant.tenant_nwc_url,
    };

    (StatusCode::OK, Json(updated)).into_response()
}

async fn admin_list_relays(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
) -> Response {
    let admin = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    if !state.admin_pubkeys.contains(&admin) {
        return forbidden();
    }

    match state.repo.list_relays().await {
        Ok(relays) => (StatusCode::OK, Json(relays)).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "failed to load relays".into(),
            }),
        )
            .into_response(),
    }
}

async fn admin_get_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(id): Path<String>,
) -> Response {
    let admin = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    if !state.admin_pubkeys.contains(&admin) {
        return forbidden();
    }

    match state.repo.get_relay(&id).await {
        Ok(Some(relay)) => (StatusCode::OK, Json(relay)).into_response(),
        Ok(None) => not_found(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "failed to load relay".into(),
            }),
        )
            .into_response(),
    }
}

async fn admin_update_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(id): Path<String>,
    Json(payload): Json<UpdateRelayRequest>,
) -> Response {
    let admin = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    if !state.admin_pubkeys.contains(&admin) {
        return forbidden();
    }

    let existing = match state.repo.get_relay(&id).await {
        Ok(Some(relay)) => relay,
        Ok(None) => return not_found(),
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "failed to load relay".into(),
                }),
            )
                .into_response();
        }
    };

    let relay = Relay {
        id: existing.id,
        tenant: existing.tenant,
        name: payload.name,
        subdomain: payload.subdomain.clone(),
        icon: payload.icon,
        description: payload.description,
        plan: payload.plan,
        status: existing.status,
        config: payload.config,
    };

    if let Err(err) = state.repo.upsert_relay(&relay).await {
        if is_unique_subdomain_violation(&err) {
            return (
                StatusCode::CONFLICT,
                Json(ApiError {
                    error: "subdomain already exists".into(),
                }),
            )
                .into_response();
        }

        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "failed to update relay".into(),
            }),
        )
            .into_response();
    }

    if let Err(err) = state.provisioner.update_relay(&relay).await {
        tracing::error!(relay_id = relay.id, error = %err, "zooid patch failed");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError { error: format!("failed to update relay config: {err}") }),
        )
            .into_response();
    }

    (StatusCode::OK, Json(relay)).into_response()
}

async fn admin_deactivate_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(id): Path<String>,
) -> Response {
    let admin = match extract_auth_pubkey(&headers, &method, &uri) {
        Ok(pubkey) => pubkey,
        Err(_) => return unauthorized(),
    };

    if !state.admin_pubkeys.contains(&admin) {
        return forbidden();
    }

    let existing = match state.repo.get_relay(&id).await {
        Ok(Some(relay)) => relay,
        Ok(None) => return not_found(),
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "failed to load relay".into(),
                }),
            )
                .into_response();
        }
    };

    let relay = Relay {
        status: "deactivated".to_string(),
        config: None,
        ..existing
    };

    if let Err(_) = state.repo.upsert_relay(&relay).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "failed to deactivate relay".into(),
            }),
        )
            .into_response();
    }

    (StatusCode::OK, Json(relay)).into_response()
}
