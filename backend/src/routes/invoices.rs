use std::sync::Arc;

use axum::extract::{Path, State};

use crate::api::{Api, AuthedPubkey};
use crate::web::{ApiResult, bad_request, internal, not_found, ok};

pub async fn list_tenant_invoices(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &pubkey)?;
    let tenant = api.get_tenant_or_404(&pubkey).await?;

    let invoices = api
        .stripe
        .list_invoices(&tenant.stripe_customer_id)
        .await
        .map_err(internal)?;
    ok(invoices)
}

pub async fn get_invoice(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> ApiResult {
    let Some(invoice) = api.stripe.get_invoice(stripe_invoice_id).await.map_err(internal)? else {
        return Err(not_found("invoice not found"));
    };

    let Some(_) = api
        .query
        .get_tenant_by_stripe_customer_id(&invoice.customer)
        .await
        .map_err(internal)?
    else {
        return Err(not_found("invoice not found"));
    };

    api.require_admin_or_tenant(auth, &tenant.pubkey)?;

    Ok((invoice, tenant))

    ok(invoice)
}

pub async fn get_lightning_invoice(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> ApiResult {
    let Some(invoice) = api.stripe.get_invoice(stripe_invoice_id).await.map_err(internal)? else {
        return Err(not_found("invoice not found"));
    };

    let Some(tenant) = api
        .query
        .get_tenant_by_stripe_customer_id(&invoice.customer)
        .await
        .map_err(internal)?
    else {
        return Err(not_found("invoice not found"));
    };

    let lightning_invoice = api
        .billing
        .ensure_lightning_invoice(&invoice.id, &tenant.pubkey, invoice.amount_due, &invoice.currency)
        .await
        .map_err(internal)?;

    ok(serde_json::json!(lightning_invoice))
}
