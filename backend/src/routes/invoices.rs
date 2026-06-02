use std::sync::Arc;

use axum::extract::{Path, State};

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

    // Implicitly reconcile an outstanding lightning invoice if we have one
    api.billing
        .reconcile_bolt11_for_invoice(&invoice)
        .await
        .map_err(internal)?;

    ok(invoice)
}

/// Return a payable Lightning invoice (bolt11) for an invoice, minting one if
/// needed and first settling it if it was already paid out of band.
pub async fn get_invoice_bolt11(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(invoice_id): Path<String>,
) -> ApiResult {
    let invoice = query::get_invoice(&invoice_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found("invoice not found"))?;

    api.require_admin_or_tenant(&auth, &invoice.tenant_pubkey)?;

    // Make sure we have a bolt11 for this invoice
    api.billing
        .ensure_bolt11_for_invoice(&invoice)
        .await
        .map_err(internal)?;

    // Check to see whether it was reconciled out of band
    let bolt11 = api
        .billing
        .reconcile_bolt11_for_invoice(&invoice)
        .await
        .map_err(internal)?;

    ok(serde_json::json!(bolt11))
}

/// The line items billed on an invoice, for rendering its contents and a
/// downloadable copy.
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
