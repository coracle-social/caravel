//! The invoices a tenant owes and the ways to pay one.

use axum::extract::{Path, Query, State};
use caravel_core::{Core, billing, hosting};
use serde::Deserialize;

use crate::auth::Authed;
use crate::response::{ApiResult, ok};

use super::TenantView;

pub async fn list_tenant_invoices(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&pubkey)?;
    ok(billing::list_invoices_for_tenant(&core, &pubkey).await?)
}

/// Bring the tenant's billing up to date without charging them, settling anything
/// they paid out of band.
pub async fn reconcile_tenant(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&pubkey)?;
    let tenant = hosting::require_tenant(&core, &pubkey).await?;
    billing::reconcile_tenant(&core, &tenant).await?;
    ok(TenantView::from(
        hosting::require_tenant(&core, &pubkey).await?,
    ))
}

/// An unsaved invoice for what the tenant owes so far this period, or null.
pub async fn get_draft_invoice(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&pubkey)?;
    let tenant = hosting::require_tenant(&core, &pubkey).await?;
    ok(billing::get_draft_invoice(&core, &tenant).await?)
}

/// The line items behind the draft invoice.
pub async fn list_draft_invoice_items(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&pubkey)?;
    ok(billing::list_outstanding_items(&core, &pubkey).await?)
}

pub async fn get_invoice(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let invoice = billing::require_invoice(&core, &id).await?;
    actor.require_pubkey_or_admin(&invoice.tenant_pubkey)?;
    ok(invoice)
}

pub async fn list_invoice_items(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let invoice = billing::require_invoice(&core, &id).await?;
    actor.require_pubkey_or_admin(&invoice.tenant_pubkey)?;
    ok(billing::list_invoice_items(&core, &id).await?)
}

/// Collect an open invoice now, without the reminder DM a failure would send.
pub async fn reconcile_invoice(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let invoice = billing::require_invoice(&core, &id).await?;
    actor.require_pubkey_or_admin(&invoice.tenant_pubkey)?;
    let tenant = hosting::require_tenant(&core, &invoice.tenant_pubkey).await?;
    billing::collect_invoice(&core, &tenant, &invoice).await?;

    ok(billing::require_invoice(&core, &id).await?)
}

/// A payable Lightning invoice for an invoice.
pub async fn ensure_invoice_bolt11(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let invoice = billing::require_invoice(&core, &id).await?;
    actor.require_pubkey_or_admin(&invoice.tenant_pubkey)?;
    ok(billing::ensure_bolt11_for_invoice(&core, &invoice).await?)
}

#[derive(Deserialize)]
pub struct CheckoutParams {
    /// Where Stripe sends the tenant after paying or canceling.
    return_url: String,
}

/// A hosted Stripe Checkout page for paying an invoice by card, which can clear a
/// 3D Secure challenge the off-session charge can't.
pub async fn ensure_invoice_checkout(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
    Query(params): Query<CheckoutParams>,
) -> ApiResult {
    let invoice = billing::require_invoice(&core, &id).await?;
    actor.require_pubkey_or_admin(&invoice.tenant_pubkey)?;
    let tenant = hosting::require_tenant(&core, &invoice.tenant_pubkey).await?;
    let checkout =
        billing::ensure_checkout_for_invoice(&core, &tenant, &invoice, &params.return_url).await?;
    ok(serde_json::json!({ "url": checkout.url }))
}
