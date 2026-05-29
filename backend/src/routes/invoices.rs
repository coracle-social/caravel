use std::sync::Arc;

use axum::extract::{Path, State};
use serde::Serialize;

use crate::api::{Api, AuthedPubkey};
use crate::models::{Intent, Invoice};
use crate::query;
use crate::web::{ApiResult, internal, not_found, ok};

/// An invoice for the client, with its lifecycle flattened to a derived `status`
/// ("open" | "paid" | "void") alongside the underlying timestamps, plus the
/// Stripe PaymentIntents that settled it (empty unless requested).
#[derive(Serialize)]
pub struct InvoiceResponse {
    pub id: String,
    pub tenant_pubkey: String,
    pub status: String,
    pub period_start: i64,
    pub period_end: i64,
    pub created_at: i64,
    pub paid_at: Option<i64>,
    pub voided_at: Option<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub intents: Vec<Intent>,
}

impl From<Invoice> for InvoiceResponse {
    fn from(i: Invoice) -> Self {
        let status = if i.is_open() {
            "open"
        } else if i.paid_at.is_some() {
            "paid"
        } else {
            "void"
        };
        InvoiceResponse {
            status: status.to_string(),
            id: i.id,
            tenant_pubkey: i.tenant_pubkey,
            period_start: i.period_start,
            period_end: i.period_end,
            created_at: i.created_at,
            paid_at: i.paid_at,
            voided_at: i.voided_at,
            intents: Vec::new(),
        }
    }
}

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

    let invoice = query::get_latest_invoice(&pubkey).await.map_err(internal)?;

    ok(invoice.map(InvoiceResponse::from))
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

    let mut response = InvoiceResponse::from(invoice);
    response.intents = query::list_intents_for_invoice(&id).await.map_err(internal)?;

    ok(response)
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
