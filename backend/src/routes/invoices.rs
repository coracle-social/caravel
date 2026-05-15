use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Response,
};

use crate::api::{Api, AuthedPubkey};
use crate::stripe::InvoiceLookupError;
use crate::web::{ApiError, err, ok};

pub async fn list_tenant_invoices(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
) -> Result<Response, ApiError> {
    api.require_admin_or_tenant(&auth, &pubkey)?;
    let tenant = api.get_tenant_or_404(&pubkey).await?;

    match api
        .billing
        .stripe_list_invoices(&tenant.stripe_customer_id)
        .await
    {
        Ok(invoices) => Ok(ok(StatusCode::OK, invoices)),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

pub async fn get_invoice(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let (invoice, tenant) = api
        .billing
        .get_invoice_with_tenant(&id)
        .await
        .map_err(map_invoice_lookup_error)?;
    api.require_admin_or_tenant(&auth, &tenant.pubkey)?;

    let invoice = api
        .billing
        .reconcile_manual_lightning_invoice(&id, &invoice)
        .await
        .map_err(map_invoice_lookup_error)?;

    Ok(ok(StatusCode::OK, invoice))
}

pub async fn get_invoice_bolt11(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let (invoice, tenant) = api
        .billing
        .get_invoice_with_tenant(&id)
        .await
        .map_err(map_invoice_lookup_error)?;
    api.require_admin_or_tenant(&auth, &tenant.pubkey)?;

    let invoice = api
        .billing
        .reconcile_manual_lightning_invoice(&id, &invoice)
        .await
        .map_err(map_invoice_lookup_error)?;

    let status = invoice["status"].as_str().unwrap_or_default();
    if status != "open" {
        return Ok(err(
            StatusCode::BAD_REQUEST,
            "invoice-not-open",
            "invoice is not open",
        ));
    }

    let amount_due = invoice["amount_due"].as_i64().unwrap_or(0);
    let currency = invoice["currency"].as_str().unwrap_or("usd");

    match api
        .billing
        .get_or_create_manual_lightning_bolt11(&id, &tenant.pubkey, amount_due, currency)
        .await
    {
        Ok(bolt11) => Ok(ok(StatusCode::OK, serde_json::json!({ "bolt11": bolt11 }))),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

fn map_invoice_lookup_error(error: InvoiceLookupError) -> ApiError {
    match error {
        InvoiceLookupError::StripeClient { status } => {
            let status = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_REQUEST);
            match status {
                StatusCode::NOT_FOUND => ApiError::NotFound("invoice not found"),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                    ApiError::Forbidden("invoice access denied")
                }
                _ => ApiError::Client {
                    status,
                    code: "invoice-request-rejected",
                    message: "invoice request rejected",
                },
            }
        }
        InvoiceLookupError::Internal(error) => ApiError::Internal(error.to_string()),
    }
}
