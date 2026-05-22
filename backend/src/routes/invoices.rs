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
    let (invoice, _tenant) = load_authorized_invoice(&api, &auth, &id).await?;

    let invoice = api
        .billing
        .reconcile_invoice(&invoice)
        .await
        .map_err(internal)?;

    ok(invoice)
}

pub async fn get_lightning_invoice(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> ApiResult {
    let (invoice, tenant) = load_authorized_invoice(&api, &auth, &id).await?;

    // Settle first: this checks the currently-stored bolt11 against the wallet,
    // so a payment that landed before expiry is always caught before we'd
    // consider regenerating below.
    let invoice = api
        .billing
        .reconcile_invoice(&invoice)
        .await
        .map_err(internal)?;

    if invoice.status != "open" {
        return Err(bad_request("invoice-not-open", "invoice is not open"));
    }

    let invoice = api
        .billing
        .ensure_lightning_invoice(&invoice.id, &tenant.pubkey, invoice.amount_due, &invoice.currency)
        .await
        .map_err(internal)?;

    ok(serde_json::json!(invoice))
}

/// Fetch a Stripe invoice and the tenant that owns it, enforcing that the
/// caller is that tenant (or an admin). Returns 404 if the invoice or its
/// tenant can't be found.
async fn load_authorized_invoice(
    api: &Api,
    auth: &str,
    stripe_invoice_id: &str,
) -> Result<(crate::stripe::StripeInvoice, crate::models::Tenant), crate::web::ApiError> {
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

    api.require_admin_or_tenant(auth, &tenant.pubkey)?;

    Ok((invoice, tenant))
}
