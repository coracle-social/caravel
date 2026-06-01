use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::api::{Api, AuthedPubkey};
use crate::models::Tenant;
use crate::web::{ApiResult, internal, map_unique_error, ok};
use crate::{command, env, query};

#[derive(Serialize)]
pub struct TenantResponse {
    pub pubkey: String,
    pub nwc_is_set: bool,
    pub nwc_error: Option<String>,
    pub stripe_error: Option<String>,
    pub created_at: i64,
    pub billing_anchor: Option<i64>,
    pub stripe_customer_id: String,
    pub stripe_payment_method_id: Option<String>,
    /// Set when billing has churned the tenant; the UI uses it to warn that the
    /// account is delinquent until billing is re-activated.
    pub churned_at: Option<i64>,
}

impl From<Tenant> for TenantResponse {
    fn from(t: Tenant) -> Self {
        TenantResponse {
            nwc_is_set: !t.nwc_url.is_empty(),
            pubkey: t.pubkey,
            nwc_error: t.nwc_error,
            stripe_error: t.stripe_error,
            created_at: t.created_at,
            billing_anchor: t.billing_anchor,
            stripe_customer_id: t.stripe_customer_id,
            stripe_payment_method_id: t.stripe_payment_method_id,
            churned_at: t.churned_at,
        }
    }
}

pub async fn list_tenants(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
) -> ApiResult {
    api.require_admin(&auth)?;

    let tenants = query::list_tenants().await.map_err(internal)?;
    ok(tenants
        .into_iter()
        .map(TenantResponse::from)
        .collect::<Vec<_>>())
}

/// Fetch a tenant by pubkey. Automatically refreshes the tenant's stripe payment data.
pub async fn get_tenant(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &pubkey)?;

    let mut tenant = api.get_tenant_or_404(&pubkey).await?;

    api.billing
        .sync_stripe_customer(&mut tenant)
        .await
        .map_err(internal)?;

    ok(TenantResponse::from(tenant))
}

/// Create the tenant row for the calling pubkey and provision its Stripe
/// customer. Idempotent: an existing tenant (including one created by a
/// concurrent unique-constraint race) is returned as-is.
pub async fn create_tenant(
    State(api): State<Arc<Api>>,
    AuthedPubkey(pubkey): AuthedPubkey,
) -> ApiResult {
    if let Some(t) = query::get_tenant(&pubkey).await.map_err(internal)? {
        return ok(TenantResponse::from(t));
    }

    let display_name = api
        .robot
        .fetch_nostr_name(&pubkey)
        .await
        .unwrap_or(pubkey.chars().take(8).collect());

    let stripe_customer_id = api
        .stripe
        .create_customer(&pubkey, &display_name)
        .await
        .map_err(internal)?;

    let tenant = Tenant {
        pubkey: pubkey.clone(),
        created_at: Utc::now().timestamp(),
        stripe_customer_id,
        ..Default::default()
    };

    match command::create_tenant(&tenant).await {
        Ok(()) => ok(TenantResponse::from(tenant)),
        Err(e) if matches!(map_unique_error(&e), Some("pubkey-exists")) => {
            match query::get_tenant(&pubkey).await {
                Ok(Some(t)) => ok(TenantResponse::from(t)),
                Ok(None) => Err(internal("tenant row missing after unique-constraint race")),
                Err(e) => Err(internal(e)),
            }
        }
        Err(e) => Err(internal(e)),
    }
}

#[derive(Deserialize)]
pub struct UpdateTenantRequest {
    pub nwc_url: Option<String>,
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
            tenant.nwc_url = env::get().encrypt(&nwc_url).map_err(internal)?;
        }
    }

    command::update_tenant(&tenant).await.map_err(internal)?;
    ok(TenantResponse::from(tenant))
}

/// List a tenant's relays.
pub async fn list_tenant_relays(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &pubkey)?;

    let relays = query::list_relays_for_tenant(&pubkey)
        .await
        .map_err(internal)?;
    ok(relays)
}

/// List a tenant's invoices after reconciling the tenant's billing state.
pub async fn list_tenant_invoices(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &pubkey)?;

    let tenant = api.get_tenant_or_404(&pubkey).await?;

    api.billing
        .reconcile_subscription(&tenant, false)
        .await
        .map_err(internal)?;

    let invoices = query::list_invoices(&pubkey).await.map_err(internal)?;

    ok(invoices)
}

#[derive(Deserialize)]
pub struct StripeSessionParams {
    return_url: Option<String>,
}

/// Create a Stripe billing-portal session for the tenant to manage their saved
/// payment methods, returning the portal URL.
pub async fn create_stripe_session(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
    Query(params): Query<StripeSessionParams>,
) -> ApiResult {
    api.require_tenant(&auth, &pubkey)?;

    let tenant = api.get_tenant_or_404(&pubkey).await?;

    let url = api
        .stripe
        .create_portal_session(&tenant.stripe_customer_id, params.return_url.as_deref())
        .await
        .map_err(internal)?;

    ok(serde_json::json!({ "url": url }))
}
