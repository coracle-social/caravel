use std::sync::Arc;

use axum::extract::{Path, State};

use crate::api::{Api, AuthedPubkey};
use crate::query;
use crate::web::{ApiResult, internal, not_found, ok};

/// The tenant's most recent invoice, after first materializing any outstanding
/// line items into a fresh one — so the frontend can collect payment right after
/// a change (e.g. creating a relay). Payment isn't attempted here; the caller
/// drives it via the bolt11/Stripe endpoints. `null` when the tenant has no
/// invoices and nothing is outstanding.
pub async fn get_tenant_latest_invoice(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &pubkey)?;
    let tenant = api.get_tenant_or_404(&pubkey).await?;

    api.billing.reconcile_subscription(&tenant).await.map_err(internal)?;

    let invoice = query::get_latest_invoice_for_tenant(&pubkey).await.map_err(internal)?;

    ok(invoice)
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

    let bolt11 = api
        .billing
        .ensure_and_reconcile_bolt11(&invoice_id)
        .await
        .map_err(internal)?;

    ok(serde_json::json!(bolt11))
}
