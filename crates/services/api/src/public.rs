//! Routes anyone may call, and the session routes that turn a signature into a
//! bearer token.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use caravel_core::{Core, domains, sessions};
use caravel_domain::billing;
use caravel_util::error::Error;
use serde::Serialize;

use crate::auth::{Authed, authorization};
use crate::response::{ApiResult, created, ok};

pub async fn list_plans() -> ApiResult {
    ok(billing::list_plans())
}

pub async fn get_plan(Path(id): Path<String>) -> ApiResult {
    ok(billing::get_plan(&id).ok_or(Error::NotFound("plan not found"))?)
}

/// On-demand TLS's ask: 200 if the platform serves `?domain=`, 404 if not.
pub async fn check_domain(
    State(core): State<Core>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let Some(domain) = params.get("domain") else {
        return StatusCode::BAD_REQUEST;
    };

    match domains::is_served_host(&core, domain).await {
        Ok(true) => StatusCode::OK,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(error) => {
            tracing::warn!(domain = %domain, error = %error, "domain check failed");
            StatusCode::NOT_FOUND
        }
    }
}

#[derive(Serialize)]
struct SessionResponse {
    pubkey: String,
    token: String,
    expires_at: i64,
}

/// Exchange a NIP-98 signature over exactly this request for a bearer token.
pub async fn create_session(State(core): State<Core>, headers: HeaderMap) -> ApiResult {
    let now = core.clock.now();
    let url = format!("{}/session", core.config.api_url.trim_end_matches('/'));
    let pubkey =
        caravel_util::auth::verify_request_auth(authorization(&headers)?, &url, "POST", now)?;
    let (token, session) = sessions::start(&core, &pubkey).await?;

    created(SessionResponse {
        pubkey,
        token,
        expires_at: session.expires_at,
    })
}

/// End the session whose bearer token authenticates this request.
pub async fn delete_session(State(core): State<Core>, headers: HeaderMap) -> ApiResult {
    let token = authorization(&headers)?
        .strip_prefix("Bearer ")
        .ok_or_else(|| Error::Unauthorized("a session token is required".into()))?;
    sessions::end(&core, token.trim()).await?;
    ok(())
}

pub async fn get_identity(Authed(actor): Authed) -> ApiResult {
    ok(actor)
}
