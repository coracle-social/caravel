use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, Query as QueryParams, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::Deserialize;

use crate::api::{Api, AuthedPubkey};
use crate::web::{ApiError, err, ok};

#[derive(Deserialize)]
pub struct StripeSessionParams {
    return_url: Option<String>,
}

pub async fn create_stripe_session(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
    QueryParams(params): QueryParams<StripeSessionParams>,
) -> Result<Response, ApiError> {
    api.require_admin_or_tenant(&auth, &pubkey)?;
    let tenant = api.get_tenant_or_404(&pubkey).await?;

    match api
        .billing
        .stripe_create_portal_session(&tenant.stripe_customer_id, params.return_url.as_deref())
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

/// Stripe webhook endpoint. Authenticated via `Stripe-Signature` verification
/// on the raw body, not via NIP-98, so it does not use `AuthedPubkey`.
pub async fn stripe_webhook(
    State(api): State<Arc<Api>>,
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

    match api.billing.handle_webhook(payload, signature).await {
        Ok(()) => ok(StatusCode::OK, ()),
        Err(e) => err(StatusCode::BAD_REQUEST, "webhook-error", &e.to_string()),
    }
}
