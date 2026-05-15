use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::api::{Api, AuthedPubkey};
use crate::models::Tenant;
use crate::web::{ApiResult, internal, map_unique_error, ok};

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
) -> ApiResult {
    api.require_admin(&auth)?;

    let tenants = api.query.list_tenants().await.map_err(internal)?;
    ok(tenants
        .into_iter()
        .map(TenantResponse::from)
        .collect::<Vec<_>>())
}

/// Creates the tenant row for the calling pubkey. Idempotent: if the tenant
/// already exists (including a unique-constraint race) we return the existing
/// row.
pub async fn create_tenant(
    State(api): State<Arc<Api>>,
    AuthedPubkey(pubkey): AuthedPubkey,
) -> ApiResult {
    if let Some(t) = api.query.get_tenant(&pubkey).await.map_err(internal)? {
        return ok(TenantResponse::from(t));
    }

    let stripe_customer_id = api
        .billing
        .stripe_create_customer(&pubkey)
        .await
        .map_err(internal)?;

    let tenant = Tenant {
        pubkey: pubkey.clone(),
        created_at: Utc::now().timestamp(),
        stripe_customer_id,
        ..Default::default()
    };

    match api.command.create_tenant(&tenant).await {
        Ok(()) => ok(TenantResponse::from(tenant)),
        Err(e) if matches!(map_unique_error(&e), Some("pubkey-exists")) => {
            match api.query.get_tenant(&pubkey).await {
                Ok(Some(t)) => ok(TenantResponse::from(t)),
                Ok(None) => Err(internal("tenant row missing after unique-constraint race")),
                Err(e) => Err(internal(e)),
            }
        }
        Err(e) => Err(internal(e)),
    }
}

pub async fn get_tenant(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &pubkey)?;
    let tenant = api.get_tenant_or_404(&pubkey).await?;
    ok(TenantResponse::from(tenant))
}

pub async fn update_tenant(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
    Json(payload): Json<UpdateTenantRequest>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &pubkey)?;
    let mut tenant = api.get_tenant_or_404(&pubkey).await?;

    // Encrypt tenant's nwc_url at rest
    if let Some(nwc_url) = payload.nwc_url {
        if nwc_url.is_empty() {
            tenant.nwc_url = String::new();
        } else {
            tenant.nwc_url = api.env.encrypt(&nwc_url).map_err(internal)?;
        }
    }

    api.command.update_tenant(&tenant).await.map_err(internal)?;
    ok(TenantResponse::from(tenant))
}

pub async fn list_tenant_relays(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &pubkey)?;

    let relays = api
        .query
        .list_relays_for_tenant(&pubkey)
        .await
        .map_err(internal)?;
    ok(relays)
}
