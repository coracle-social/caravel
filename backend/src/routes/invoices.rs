use std::sync::Arc;

use axum::extract::{Path, Query, State};
use serde::Deserialize;

use crate::api::{Api, AuthedPubkey};
use crate::query;
use crate::web::{ApiResult, internal, not_found, ok};

pub async fn list_invoices(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
) -> ApiResult {
    api.require_admin(&auth)?;

    ok(query::list_invoices().await.map_err(internal)?)
}

/// Read a single invoice
pub async fn get_invoice(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> ApiResult {
    let invoice = query::get_invoice(&id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found("invoice not found"))?;

    api.require_admin_or_tenant(&auth, &invoice.tenant_pubkey)?;

    ok(invoice)
}

/// Reconcile and collect an open invoice
pub async fn reconcile_invoice(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> ApiResult {
    let invoice = query::get_invoice(&id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found("invoice not found"))?;

    api.require_admin_or_tenant(&auth, &invoice.tenant_pubkey)?;

    // Nothing to collect on an already-resolved invoice.
    if invoice.paid_at.is_some() || invoice.voided_at.is_some() {
        return ok(invoice);
    }

    let tenant = api.get_tenant_or_404(&invoice.tenant_pubkey).await?;

    api.billing
        .ensure_bolt11_for_invoice(&invoice)
        .await
        .map_err(internal)?;

    api.billing
        .reconcile_payments(&tenant, &invoice, true, false)
        .await
        .map_err(internal)?;

    // Re-read so the caller sees the possibly now-paid invoice.
    let invoice = query::get_invoice(&id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found("invoice not found"))?;

    ok(invoice)
}

/// Idempotently create a payable Lightning invoice (bolt11)
pub async fn ensure_invoice_bolt11(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(invoice_id): Path<String>,
) -> ApiResult {
    let invoice = query::get_invoice(&invoice_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found("invoice not found"))?;

    api.require_admin_or_tenant(&auth, &invoice.tenant_pubkey)?;

    let bolt11 = api
        .billing
        .ensure_bolt11_for_invoice(&invoice)
        .await
        .map_err(internal)?;

    ok(bolt11)
}

#[derive(Deserialize)]
pub struct CheckoutParams {
    /// Where Stripe returns the tenant after success or cancel. Required, since
    /// the server has no way to guess where the caller wants to land — a native
    /// app passes a deep link, the web app the page it opened checkout from.
    return_url: String,
}

/// Open a hosted Stripe Checkout session to pay a single open invoice by card,
/// returning the URL to redirect the tenant to. Unlike the off-session card
/// charge, Checkout can satisfy a 3D Secure authentication challenge; the
/// resulting payment is reconciled by `reconcile_invoice` (or the dunning poll).
/// Requires a `?return_url=` naming the page Stripe sends the tenant back to.
pub async fn ensure_invoice_checkout(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
    Query(params): Query<CheckoutParams>,
) -> ApiResult {
    let invoice = query::get_invoice(&id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found("invoice not found"))?;

    api.require_admin_or_tenant(&auth, &invoice.tenant_pubkey)?;

    let tenant = api.get_tenant_or_404(&invoice.tenant_pubkey).await?;

    let checkout = api
        .billing
        .ensure_checkout_for_invoice(&tenant, &invoice, &params.return_url)
        .await
        .map_err(internal)?;

    ok(serde_json::json!({ "url": checkout.url }))
}

/// The line items billed on an invoice
pub async fn list_invoice_items(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(invoice_id): Path<String>,
) -> ApiResult {
    let invoice = query::get_invoice(&invoice_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found("invoice not found"))?;

    api.require_admin_or_tenant(&auth, &invoice.tenant_pubkey)?;

    let items = query::list_invoice_items_for_invoice(&invoice_id)
        .await
        .map_err(internal)?;

    ok(items)
}
