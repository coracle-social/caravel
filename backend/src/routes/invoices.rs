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
    let Some(invoice) = self.stripe.get_invoice(id).await? else {
        return not_found("invoice not found")
    };

    let tenant = api
        .query
        .get_tenant_by_stripe_customer_id(&invoice.customer)
        .await?
        .ok_or_else(|| anyhow!("invoice not found"))?;

    api.require_admin_or_tenant(&auth, &tenant.pubkey)?;

    let invoice = api
        .billing
        .reconcile_manual_lightning_invoice(&id, &invoice)
        .await
        .map_err(internal)?;

    ok(invoice)
}

pub async fn get_invoice_bolt11(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> ApiResult {
    let Some(invoice) = self.stripe.get_invoice(id).await? else {
        return not_found("invoice not found")
    };

    let tenant = api
        .query
        .get_tenant_by_stripe_customer_id(&invoice.customer)
        .await?
        .ok_or_else(|| anyhow!("invoice not found"))?;

    api.require_admin_or_tenant(&auth, &tenant.pubkey)?;

    let invoice = api
        .billing
        .reconcile_manual_lightning_invoice(&id, &invoice)
        .await
        .map_err(internal)?;

    if invoice.status != "open" {
        return Err(bad_request("invoice-not-open", "invoice is not open"));
    }

    let bolt11 = api
        .billing
        .get_or_create_manual_lightning_bolt11(
            &id,
            &tenant.pubkey,
            invoice.amount_due,
            &invoice.currency,
        )
        .await
        .map_err(internal)?;
    ok(serde_json::json!({ "bolt11": bolt11 }))
}
