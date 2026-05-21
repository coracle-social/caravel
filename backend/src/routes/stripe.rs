use std::sync::Arc;

use anyhow::Result;
use axum::{
    body::Bytes,
    extract::{Path, Query as QueryParams, State},
    http::HeaderMap,
};
use serde::Deserialize;

use crate::api::{Api, AuthedPubkey};
use crate::billing::NwcInvoicePaymentOutcome;
use crate::models::{RELAY_STATUS_ACTIVE, RELAY_STATUS_DELINQUENT};
use crate::web::{ApiResult, bad_request, internal, ok};

const MANUAL_LIGHTNING_PAYMENT_DM: &str = "Payment is due for your relay subscription. Please visit the application to complete a manual Lightning payment.";
const NWC_ERROR_DM_PREFIX: &str = "NWC auto-payment failed:";
const NWC_ERROR_DM_MAX_CHARS: usize = 240;

#[derive(Deserialize)]
pub struct StripeSessionParams {
    return_url: Option<String>,
}

pub async fn create_stripe_session(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(pubkey): Path<String>,
    QueryParams(params): QueryParams<StripeSessionParams>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &pubkey)?;
    let tenant = api.get_tenant_or_404(&pubkey).await?;

    let url = api
        .stripe
        .create_portal_session(&tenant.stripe_customer_id, params.return_url.as_deref())
        .await
        .map_err(internal)?;
    ok(serde_json::json!({ "url": url }))
}

/// Stripe webhook endpoint. Authenticated via `Stripe-Signature` verification
/// on the raw body, not via NIP-98, so it does not use `AuthedPubkey`.
pub async fn stripe_webhook(
    State(api): State<Arc<Api>>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult {
    let signature = headers
        .get("Stripe-Signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let payload = std::str::from_utf8(&body)
        .map_err(|_| bad_request("bad-request", "invalid payload"))?;

    handle_webhook(&api, payload, signature)
        .await
        .map_err(|e| bad_request("webhook-error", &e.to_string()))?;
    ok(())
}

// --- Webhook event handlers ---
//
// These translate verified Stripe events into domain actions. The Stripe HTTP
// calls and Lightning/NWC payment orchestration they invoke live in
// [`crate::stripe`] and [`crate::billing`] respectively.

async fn handle_webhook(api: &Api, payload: &str, signature: &str) -> Result<()> {
    let event = api.stripe.get_webhook_event(payload, signature)?;
    let obj = &event.data.object;

    match event.event_type.as_str() {
        "invoice.created" => {
            let customer = obj["customer"].as_str().unwrap_or_default();
            let amount_due = obj["amount_due"].as_i64().unwrap_or(0);
            let currency = obj["currency"].as_str().unwrap_or("usd");
            let invoice_id = obj["id"].as_str().unwrap_or_default();
            handle_invoice_created(api, customer, amount_due, currency, invoice_id).await?;
        }
        "invoice.paid" => {
            let customer = obj["customer"].as_str().unwrap_or_default();
            handle_invoice_paid(api, customer).await?;
        }
        "invoice.payment_failed" => {
            let customer = obj["customer"].as_str().unwrap_or_default();
            handle_invoice_payment_failed(api, customer).await?;
        }
        "invoice.overdue" => {
            let customer = obj["customer"].as_str().unwrap_or_default();
            handle_invoice_overdue(api, customer).await?;
        }
        "customer.subscription.updated" => {
            let customer = obj["customer"].as_str().unwrap_or_default();
            let status = obj["status"].as_str().unwrap_or_default();
            handle_subscription_updated(api, customer, status).await?;
        }
        "customer.subscription.deleted" => {
            let customer = obj["customer"].as_str().unwrap_or_default();
            handle_subscription_deleted(api, customer).await?;
        }
        "payment_method.attached" => {
            let customer = obj["customer"].as_str().unwrap_or_default();
            handle_payment_method_attached(api, customer).await?;
        }
        _ => {}
    }

    Ok(())
}

async fn handle_invoice_created(
    api: &Api,
    stripe_customer_id: &str,
    amount_due: i64,
    currency: &str,
    invoice_id: &str,
) -> Result<()> {
    if amount_due == 0 {
        return Ok(());
    }

    let Some(tenant) = api
        .query
        .get_tenant_by_stripe_customer_id(stripe_customer_id)
        .await?
    else {
        return Ok(());
    };

    let mut nwc_error_for_dm: Option<String> = None;

    // 1. NWC auto-pay: if the tenant has a nwc_url
    if !tenant.nwc_url.is_empty() {
        let plain_nwc_url = api.env.decrypt(&tenant.nwc_url)?;
        match api
            .billing
            .nwc_pay_invoice(
                invoice_id,
                &tenant.pubkey,
                amount_due,
                currency,
                &plain_nwc_url,
            )
            .await?
        {
            NwcInvoicePaymentOutcome::Paid => {
                api.billing
                    .mark_invoice_paid_out_of_band_after_nwc(invoice_id, &tenant.pubkey)
                    .await?;
                return Ok(());
            }
            NwcInvoicePaymentOutcome::Fallback(e) => {
                let error_msg = format!("{e}");
                api.command
                    .set_tenant_nwc_error(&tenant.pubkey, &error_msg)
                    .await?;
                tracing::warn!(
                    error = %e,
                    tenant_pubkey = %tenant.pubkey,
                    stripe_customer_id,
                    invoice_id,
                    "nwc auto-payment failed for invoice.created"
                );
                nwc_error_for_dm = summarize_nwc_error_for_dm(&error_msg);
                // Fall through to next option
            }
            NwcInvoicePaymentOutcome::Pending(e) => {
                let error_msg = format!("{e}");
                api.command
                    .set_tenant_nwc_error(&tenant.pubkey, &error_msg)
                    .await?;
                tracing::error!(
                    error = %e,
                    tenant_pubkey = %tenant.pubkey,
                    stripe_customer_id,
                    invoice_id,
                    "nwc auto-payment requires reconciliation before retry"
                );
                return Err(e);
            }
        }
    }

    // 2. Card on file: if the tenant has a payment method, Stripe charges automatically
    if api
        .stripe
        .has_payment_method(&tenant.stripe_customer_id)
        .await?
    {
        return Ok(());
    }

    // 3. Manual payment: send a DM
    let dm_message = manual_lightning_payment_dm(nwc_error_for_dm.as_deref());
    api.robot.send_dm(&tenant.pubkey, &dm_message).await?;

    Ok(())
}

async fn handle_invoice_paid(api: &Api, stripe_customer_id: &str) -> Result<()> {
    let Some(tenant) = api
        .query
        .get_tenant_by_stripe_customer_id(stripe_customer_id)
        .await?
    else {
        return Ok(());
    };

    if tenant.past_due_at.is_some() {
        api.command.clear_tenant_past_due(&tenant.pubkey).await?;

        let relays = api.query.list_relays_for_tenant(&tenant.pubkey).await?;
        for relay in relays {
            if relay.status == RELAY_STATUS_DELINQUENT && api.query.is_paid_plan(&relay.plan) {
                api.command.activate_relay(&relay).await?;
            }
        }
    }

    Ok(())
}

async fn handle_invoice_payment_failed(api: &Api, stripe_customer_id: &str) -> Result<()> {
    let Some(tenant) = api
        .query
        .get_tenant_by_stripe_customer_id(stripe_customer_id)
        .await?
    else {
        return Ok(());
    };

    if tenant.past_due_at.is_none() {
        api.command.set_tenant_past_due(&tenant.pubkey).await?;
        api.robot
            .send_dm(
                &tenant.pubkey,
                "Your payment has failed. Your relays may be deactivated if not resolved within a week.",
            )
            .await?;
    }

    Ok(())
}

async fn handle_subscription_updated(
    api: &Api,
    stripe_customer_id: &str,
    status: &str,
) -> Result<()> {
    if status != "canceled" && status != "unpaid" {
        return Ok(());
    }

    let Some(tenant) = api
        .query
        .get_tenant_by_stripe_customer_id(stripe_customer_id)
        .await?
    else {
        return Ok(());
    };

    api.command
        .clear_tenant_subscription(&tenant.pubkey)
        .await?;

    let relays = api.query.list_relays_for_tenant(&tenant.pubkey).await?;
    for relay in relays {
        if relay.status == RELAY_STATUS_ACTIVE && api.query.is_paid_plan(&relay.plan) {
            api.command.mark_relay_delinquent(&relay).await?;
        }
    }

    Ok(())
}

async fn handle_subscription_deleted(api: &Api, stripe_customer_id: &str) -> Result<()> {
    let Some(tenant) = api
        .query
        .get_tenant_by_stripe_customer_id(stripe_customer_id)
        .await?
    else {
        return Ok(());
    };

    api.command
        .clear_tenant_subscription(&tenant.pubkey)
        .await?;

    Ok(())
}

async fn handle_invoice_overdue(api: &Api, stripe_customer_id: &str) -> Result<()> {
    let Some(tenant) = api
        .query
        .get_tenant_by_stripe_customer_id(stripe_customer_id)
        .await?
    else {
        return Ok(());
    };

    let relays = api.query.list_relays_for_tenant(&tenant.pubkey).await?;
    for relay in relays {
        if relay.status == RELAY_STATUS_ACTIVE && api.query.is_paid_plan(&relay.plan) {
            api.command.mark_relay_delinquent(&relay).await?;
        }
    }

    api.robot
        .send_dm(
            &tenant.pubkey,
            "Your paid relays have been deactivated due to non-payment.",
        )
        .await?;

    Ok(())
}

async fn handle_payment_method_attached(api: &Api, stripe_customer_id: &str) -> Result<()> {
    if stripe_customer_id.is_empty() {
        return Ok(());
    }

    let Some(tenant) = api
        .query
        .get_tenant_by_stripe_customer_id(stripe_customer_id)
        .await?
    else {
        return Ok(());
    };

    api.billing.pay_outstanding_card_invoices(&tenant).await?;
    Ok(())
}

fn summarize_nwc_error_for_dm(error: &str) -> Option<String> {
    let normalized = error.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return None;
    }

    if normalized.chars().count() <= NWC_ERROR_DM_MAX_CHARS {
        return Some(normalized);
    }

    let prefix_len = NWC_ERROR_DM_MAX_CHARS.saturating_sub(3);
    let mut truncated = normalized.chars().take(prefix_len).collect::<String>();
    truncated.push_str("...");
    Some(truncated)
}

fn manual_lightning_payment_dm(nwc_error: Option<&str>) -> String {
    match nwc_error {
        Some(error) if !error.is_empty() => {
            format!("{MANUAL_LIGHTNING_PAYMENT_DM}\n\n{NWC_ERROR_DM_PREFIX} {error}")
        }
        _ => MANUAL_LIGHTNING_PAYMENT_DM.to_string(),
    }
}
