//! The Stripe Connect account a tenant is paid their memberships through.

use axum::Json;
use axum::extract::{Path, State};
use caravel_core::{Core, hosting, payouts};

use crate::ReturnTo;
use crate::auth::Authed;
use crate::response::{ApiResult, ok};

/// Whether the tenant can be paid by their members.
pub async fn get_payouts(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&pubkey)?;
    let tenant = hosting::require_tenant(&core, &pubkey).await?;
    ok(payouts::refresh_status(&core, &tenant).await?)
}

/// A Stripe onboarding link, for the tenant to set up being paid, which returns
/// them to `return_url` or else their account page.
pub async fn onboard_payouts(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
    Json(request): Json<ReturnTo>,
) -> ApiResult {
    actor.require_pubkey(&pubkey)?;
    let tenant = hosting::require_tenant(&core, &pubkey).await?;
    let return_url = request
        .return_url
        .unwrap_or_else(|| tenant.return_url.clone());
    ok(serde_json::json!({ "url": payouts::onboard(&core, &tenant, &return_url).await? }))
}

/// A login link to the tenant's payouts dashboard on Stripe.
pub async fn open_payouts_dashboard(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
) -> ApiResult {
    actor.require_pubkey(&pubkey)?;
    let tenant = hosting::require_tenant(&core, &pubkey).await?;
    ok(serde_json::json!({ "url": payouts::dashboard(&core, &tenant).await? }))
}
