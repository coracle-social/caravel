use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::api::{Api, AuthedPubkey};
use crate::billing::BillingPeriod;
use crate::models::{Invoice, Tenant};
use crate::web::{ApiResult, bad_request, internal, map_unique_error, ok};
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
    /// Set when an admin has banned the tenant: relay creation is blocked,
    /// billing is canceled, and all of its relays have been purged.
    pub banned_at: Option<i64>,
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
            banned_at: t.banned_at,
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

/// Fetch a tenant by pubkey.
pub async fn get_tenant(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &pubkey)?;

    let tenant = api.get_tenant_or_404(&pubkey).await?;

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

/// Ban a tenant (admin only): block relay creation, cancel billing, and purge
/// all of the tenant's relays.
pub async fn ban_tenant(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> ApiResult {
    api.require_admin(&auth)?;

    let tenant = api.get_tenant_or_404(&pubkey).await?;

    if tenant.banned_at.is_some() {
        return Err(bad_request("tenant-is-banned", "tenant is already banned"));
    }

    let relays = query::list_relays_for_tenant(&pubkey)
        .await
        .map_err(internal)?;

    command::ban_tenant(&pubkey, &relays)
        .await
        .map_err(internal)?;

    let tenant = api.get_tenant_or_404(&pubkey).await?;
    ok(TenantResponse::from(tenant))
}

/// Lift a tenant's ban (admin only). Purged relays are not restored.
pub async fn unban_tenant(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> ApiResult {
    api.require_admin(&auth)?;

    let tenant = api.get_tenant_or_404(&pubkey).await?;

    if tenant.banned_at.is_none() {
        return Err(bad_request("tenant-not-banned", "tenant is not banned"));
    }

    command::unban_tenant(&pubkey).await.map_err(internal)?;

    let tenant = api.get_tenant_or_404(&pubkey).await?;
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

/// List a tenant's invoices.
pub async fn list_tenant_invoices(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &pubkey)?;

    let invoices = query::list_invoices_for_tenant(&pubkey)
        .await
        .map_err(internal)?;

    ok(invoices)
}

/// Reconcile a tenant's subscription
pub async fn reconcile_tenant(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &pubkey)?;

    let tenant = api.get_tenant_or_404(&pubkey).await?;

    api.billing
        .sync_stripe_customer(&tenant)
        .await
        .map_err(internal)?;

    api.billing
        .reconcile_subscription(&tenant, false)
        .await
        .map_err(internal)?;

    // Re-read so the response reflects the synced method and any billing anchor.
    let tenant = api.get_tenant_or_404(&pubkey).await?;

    ok(TenantResponse::from(tenant))
}

/// Return the tenant's draft invoice: a synthetic, unsaved `Invoice` summing the
/// outstanding line items for the current period. It mirrors what `create_invoice`
/// would bill once the balance turns positive.
pub async fn get_draft_invoice(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &pubkey)?;

    let tenant = api.get_tenant_or_404(&pubkey).await?;

    let draft = match BillingPeriod::current(&tenant) {
        Some(period) => {
            let items = query::list_unbilled_invoice_items(&pubkey)
                .await
                .map_err(internal)?;
            if items.is_empty() {
                None
            } else {
                Some(Invoice {
                    id: "draft".to_string(),
                    amount: items.iter().map(|item| item.amount).sum(),
                    tenant_pubkey: tenant.pubkey,
                    period_start: period.start,
                    period_end: period.end,
                    created_at: Utc::now().timestamp(),
                    paid_at: None,
                    voided_at: None,
                    notified_at: None,
                    method: None,
                })
            }
        }
        None => None,
    };

    ok(draft)
}

/// The outstanding line items behind a tenant's draft invoice — the current
/// period's not-yet-billed charges. Mirrors `list_invoice_items` for a real
/// invoice (the draft's sentinel id can't be looked up there) so the UI can
/// itemize the draft in the same PDF.
pub async fn list_draft_invoice_items(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &pubkey)?;

    let items = query::list_unbilled_invoice_items(&pubkey)
        .await
        .map_err(internal)?;

    ok(items)
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
