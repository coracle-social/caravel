use std::collections::HashMap;

use axum::{extract::Query, http::StatusCode, response::IntoResponse};

use crate::env;
use crate::query;

/// Caddy on-demand TLS "ask" endpoint. Returns 200 if Caddy should provision a
/// cert for the domain, 404 if not. No authentication: Caddy calls this
/// internally before issuing a certificate, passing the domain as a `?domain=`
/// query parameter.
pub async fn check_domain(Query(params): Query<HashMap<String, String>>) -> impl IntoResponse {
    let Some(domain) = params.get("domain") else {
        return StatusCode::BAD_REQUEST;
    };

    let domain = domain.trim().trim_end_matches('.').to_lowercase();

    if domain == env::get().zooid_api_host {
        return StatusCode::OK;
    }

    match query::get_relay_by_host(&domain).await {
        Ok(Some(_)) => StatusCode::OK,
        Ok(None) => StatusCode::NOT_FOUND,
        Err(e) => {
            tracing::warn!(domain = %domain, error = %e, "domain check failed");
            StatusCode::NOT_FOUND
        }
    }
}
