//! A tenant's own account: signing up, its settings, and the Stripe portal.

use axum::Json;
use axum::extract::{Path, Query, State};
use caravel_core::{Core, billing, hosting};
use serde::Deserialize;

use crate::ReturnTo;
use crate::auth::Authed;
use crate::response::{ApiResult, ok};

use super::TenantView;

#[derive(Deserialize)]
pub struct CreateTenant {
    /// The calling client's account page, where every link the robot DMs the
    /// tenant points.
    return_url: String,
}

/// Become a tenant. Idempotent: an existing tenant is returned as it is.
pub async fn create_tenant(
    State(core): State<Core>,
    Authed(actor): Authed,
    Json(body): Json<CreateTenant>,
) -> ApiResult {
    let tenant = hosting::ensure_tenant(&core, &actor.pubkey, &body.return_url).await?;
    ok(TenantView::from(tenant))
}

pub async fn get_tenant(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&pubkey)?;
    ok(TenantView::from(
        hosting::require_tenant(&core, &pubkey).await?,
    ))
}

#[derive(Deserialize)]
pub struct UpdateTenant {
    nwc_url: Option<String>,
}

pub async fn update_tenant(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
    Json(body): Json<UpdateTenant>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&pubkey)?;
    hosting::require_tenant(&core, &pubkey).await?;

    if let Some(nwc_url) = body.nwc_url {
        hosting::set_tenant_nwc_url(&core, &pubkey, &nwc_url).await?;
    }

    ok(TenantView::from(
        hosting::require_tenant(&core, &pubkey).await?,
    ))
}

pub async fn list_tenant_relays(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&pubkey)?;
    ok(hosting::list_relays_for_tenant(&core, &pubkey).await?)
}

/// A Stripe billing portal for the tenant to manage their saved cards.
pub async fn create_stripe_session(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
    Query(params): Query<ReturnTo>,
) -> ApiResult {
    actor.require_pubkey(&pubkey)?;
    let tenant = hosting::require_tenant(&core, &pubkey).await?;
    let url = billing::open_billing_portal(&core, &tenant, params.return_url.as_deref()).await?;
    ok(serde_json::json!({ "url": url }))
}
