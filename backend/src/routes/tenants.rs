use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::Response,
};
use serde::{Deserialize, Serialize};
use chrono::Utc;

use crate::api::{Api, AuthedPubkey};
use crate::models::Tenant;
use crate::web::{ApiError, err, map_unique_error, ok};

#[derive(Serialize)]
pub struct TenantResponse {
    pub pubkey: String,
    pub nwc_is_set: bool,
    pub nwc_error: Option<String>,
    pub created_at: i64,
    pub stripe_customer_id: String,
    pub stripe_subscription_id: Option<String>,
    pub past_due_at: Option<i64>,
}

impl From<Tenant> for TenantResponse {
    fn from(t: Tenant) -> Self {
        TenantResponse {
            nwc_is_set: !t.nwc_url.is_empty(),
            pubkey: t.pubkey,
            nwc_error: t.nwc_error,
            created_at: t.created_at,
            stripe_customer_id: t.stripe_customer_id,
            stripe_subscription_id: t.stripe_subscription_id,
            past_due_at: t.past_due_at,
        }
    }
}

#[derive(Deserialize)]
pub struct UpdateTenantRequest {
    pub nwc_url: Option<String>,
}

pub async fn list_tenants(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
) -> Result<Response, ApiError> {
    api.require_admin(&auth)?;

    match api.query.list_tenants().await {
        Ok(tenants) => Ok(ok(
            StatusCode::OK,
            tenants
                .into_iter()
                .map(TenantResponse::from)
                .collect::<Vec<_>>(),
        )),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

/// Creates the tenant row for the calling pubkey. Idempotent: if the tenant
/// already exists (including a unique-constraint race) we return the existing
/// row.
pub async fn create_tenant(
    State(api): State<Arc<Api>>,
    AuthedPubkey(pubkey): AuthedPubkey,
) -> Result<Response, ApiError> {
    match api.query.get_tenant(&pubkey).await {
        Ok(Some(t)) => Ok(ok(StatusCode::OK, TenantResponse::from(t))),
        Ok(None) => {
            let stripe_customer_id = match api.billing.stripe_create_customer(&pubkey).await {
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
                created_at: Utc::now().timestamp(),
                stripe_customer_id,
                stripe_subscription_id: None,
                past_due_at: None,
            };

            match api.command.create_tenant(&tenant).await {
                Ok(()) => Ok(ok(StatusCode::OK, TenantResponse::from(tenant))),
                Err(e) if matches!(map_unique_error(&e), Some("pubkey-exists")) => {
                    match api.query.get_tenant(&pubkey).await {
                        Ok(Some(t)) => Ok(ok(StatusCode::OK, TenantResponse::from(t))),
                        Ok(None) => Ok(err(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "internal",
                            "tenant row missing after unique-constraint race",
                        )),
                        Err(e) => Ok(err(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "internal",
                            &e.to_string(),
                        )),
                    }
                }
                Err(e) => Ok(err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    &e.to_string(),
                )),
            }
        }
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

pub async fn get_tenant(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> Result<Response, ApiError> {
    api.require_admin_or_tenant(&auth, &pubkey)?;
    let tenant = api.get_tenant_or_404(&pubkey).await?;
    Ok(ok(StatusCode::OK, TenantResponse::from(tenant)))
}

pub async fn update_tenant(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
    Json(payload): Json<UpdateTenantRequest>,
) -> Result<Response, ApiError> {
    api.require_admin_or_tenant(&auth, &pubkey)?;
    let mut tenant = api.get_tenant_or_404(&pubkey).await?;

    let nwc_previously_empty = tenant.nwc_url.is_empty();
    if let Some(nwc_url) = payload.nwc_url {
        if nwc_url.is_empty() {
            tenant.nwc_url = String::new();
        } else {
            tenant.nwc_url = api
                .env
                .encrypt(&nwc_url)
                .map_err(|e| ApiError::Internal(e.to_string()))?;
        }
    }

    match api.command.update_tenant(&tenant).await {
        Ok(()) => {
            // When NWC is first connected, attempt to pay any outstanding open invoices.
            if nwc_previously_empty && !tenant.nwc_url.is_empty() {
                let billing = api.billing.clone();
                let tenant_clone = tenant.clone();
                tokio::spawn(async move {
                    if let Err(e) = billing.pay_outstanding_nwc_invoices(&tenant_clone).await {
                        tracing::error!(
                            error = %e,
                            pubkey = %tenant_clone.pubkey,
                            "pay_outstanding_nwc_invoices failed after NWC setup"
                        );
                    }
                });
            }
            Ok(ok(StatusCode::OK, TenantResponse::from(tenant)))
        }
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

pub async fn list_tenant_relays(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> Result<Response, ApiError> {
    api.require_admin_or_tenant(&auth, &pubkey)?;

    match api.query.list_relays_for_tenant(&pubkey).await {
        Ok(relays) => Ok(ok(StatusCode::OK, relays)),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}
