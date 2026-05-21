use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, Query as QueryParams, State},
    http::HeaderMap,
};
use serde::Deserialize;

use crate::api::{Api, AuthedPubkey};
use crate::web::{ApiResult, bad_request, internal, ok};

#[derive(Deserialize)]
pub struct StripeSessionParams {
    return_url: Option<String>,
}

pub async fn create_stripe_session(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
    QueryParams(params): QueryParams<StripeSessionParams>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &pubkey)?;
    let tenant = api.get_tenant_or_404(&pubkey).await?;

    let url = api
        .stripe
        .create_portal_session(&tenant.stripe_customer_id, params.return_url.as_deref())
        .await
        .map_err(internal)?;
    ok(serde_json::json!({ "url": url }))
}

/// Stripe webhook endpoint. Authenticated via `Stripe-Signature` verification
/// on the raw body, not via NIP-98, so it does not use `AuthedPubkey`.
pub async fn stripe_webhook(
    State(api): State<Arc<Api>>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult {
    let signature = headers
        .get("Stripe-Signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let payload = std::str::from_utf8(&body)
        .map_err(|_| bad_request("bad-request", "invalid payload"))?;

    api.billing
        .handle_webhook(payload, signature)
        .await
        .map_err(|e| bad_request("webhook-error", &e.to_string()))?;
    ok(())
}
