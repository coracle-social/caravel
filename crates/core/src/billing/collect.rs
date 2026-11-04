//! Collecting an open invoice: Lightning, then the card on file, then a
//! reminder, and the Stripe and wallet calls each needs.

use caravel_billing::tenants::Tenant;
use caravel_billing::tenants::{Bolt11, Checkout, Invoice, InvoiceMethod};
use caravel_util::error::{Error, Result};
use caravel_util::stripe::Charge;
use caravel_util::{bitcoin, wallet::Wallet};

use crate::Core;

use super::{Collection, reactivate_tenant};

/// Hold the manual-payment DM until an open invoice is at least this old, since a
/// fresh invoice is shown to the tenant in-app first.
const FRESH_INVOICE_DM_GRACE_SECS: i64 = 24 * 60 * 60;

const MANUAL_PAYMENT_DM_INTERVAL_SECS: i64 = 12 * 24 * 60 * 60;

const MANUAL_PAYMENT_DM: &str = "Payment is due for your relay subscription. Open the link below to review the invoice and pay by Lightning or card:";

// --- Payments ---

/// Collect an invoice as far as `collection` goes. Payments that may already be in
/// flight, a Lightning invoice or a Checkout session the tenant paid, are settled
/// first, so a fresh charge is never made on top of one; when one's state can't be
/// read, the pass stops rather than charge on a guess. A failed charge's error is
/// stored on the tenant but never stops the cascade.
pub(super) async fn reconcile_payments(
    core: &Core,
    tenant: &Tenant,
    invoice: &Invoice,
    collection: Collection,
) -> Result<()> {
    let now = core.clock.now();

    if let Some(bolt11) = caravel_billing::tenants::queries::billing::get_bolt11_for_invoice(
        core.db.pool(),
        &invoice.id,
    )
    .await?
        && bolt11.settled_at.is_none()
        && core.wallet.is_settled(&bolt11.lnbc, now).await?
    {
        core.db
            .with_tx(async |conn| {
                caravel_billing::tenants::commands::billing::mark_bolt11_settled(
                    &mut *conn, &bolt11.id, now,
                )
                .await?;
                caravel_billing::tenants::commands::billing::mark_invoice_paid(
                    &mut *conn,
                    &invoice.id,
                    InvoiceMethod::Oob,
                    now,
                )
                .await
            })
            .await?;
        return cleanup_pending_payments(core, invoice).await;
    }

    if let Some(checkout) = caravel_billing::tenants::queries::billing::get_checkout_for_invoice(
        core.db.pool(),
        &invoice.id,
    )
    .await?
        && checkout.settled_at.is_none()
        && core.stripe.is_checkout_paid(&checkout.session_id).await?
    {
        core.db
            .with_tx(async |conn| {
                caravel_billing::tenants::commands::set_tenant_stripe_error(
                    &mut *conn,
                    &tenant.pubkey,
                    None,
                )
                .await?;
                caravel_billing::tenants::commands::billing::mark_checkout_settled(
                    &mut *conn,
                    &checkout.id,
                    now,
                )
                .await?;
                caravel_billing::tenants::commands::billing::mark_invoice_paid(
                    &mut *conn,
                    &invoice.id,
                    InvoiceMethod::Stripe,
                    now,
                )
                .await
            })
            .await?;
        return cleanup_pending_payments(core, invoice).await;
    }

    if collection == Collection::Settle {
        return Ok(());
    }

    if let Some(nwc_url) = &tenant.nwc_url
        && pay_with_nwc(core, tenant, nwc_url, invoice).await.is_ok()
    {
        return cleanup_pending_payments(core, invoice).await;
    }

    if let Some(payment_method_id) = &tenant.stripe_payment_method_id
        && pay_with_card(core, tenant, invoice, payment_method_id)
            .await
            .is_ok()
    {
        return cleanup_pending_payments(core, invoice).await;
    }

    if collection == Collection::ChargeAndRemind
        && let Err(error) = send_payment_reminder(core, tenant, invoice).await
    {
        tracing::error!(tenant = %tenant.pubkey, error = %error, "failed to send manual payment DM");
    }

    Ok(())
}

/// Pay an invoice from the tenant's wallet, whose url is stored encrypted.
pub(super) async fn pay_with_nwc(
    core: &Core,
    tenant: &Tenant,
    encrypted_nwc_url: &str,
    invoice: &Invoice,
) -> Result<()> {
    let result: Result<()> = async {
        let nwc_url = core.config.platform_key.decrypt(encrypted_nwc_url)?;
        let tenant_wallet = Wallet::from_url(&nwc_url)?;
        let bolt11 = ensure_bolt11_for_invoice(core, invoice).await?;
        let now = core.clock.now();

        // A payment that errors but landed anyway, or an invoice paid meanwhile, is fine.
        if let Err(error) = tenant_wallet.pay_invoice(&bolt11.lnbc, now).await
            && !core.wallet.is_settled(&bolt11.lnbc, now).await.unwrap_or(false)
            && !matches!(caravel_billing::tenants::queries::billing::get_invoice(core.db.pool(), &invoice.id).await, Ok(Some(paid)) if paid.paid_at.is_some())
        {
            return Err(error.into());
        }

        core.db
            .with_tx(async |conn| {
                caravel_billing::tenants::commands::set_tenant_nwc_error(&mut *conn, &tenant.pubkey, None).await?;
                caravel_billing::tenants::commands::billing::mark_bolt11_settled(&mut *conn, &bolt11.id, now).await?;
                caravel_billing::tenants::commands::billing::mark_invoice_paid(&mut *conn, &invoice.id, InvoiceMethod::Nwc, now).await
            })
            .await
    }
    .await;

    if let Err(error) = &result {
        caravel_billing::tenants::commands::set_tenant_nwc_error(
            core.db.pool(),
            &tenant.pubkey,
            Some(&error.to_string()),
        )
        .await?;
    }

    result
}

pub(super) async fn pay_with_card(
    core: &Core,
    tenant: &Tenant,
    invoice: &Invoice,
    payment_method_id: &str,
) -> Result<()> {
    let intent = caravel_billing::tenants::commands::billing::ensure_pending_intent(
        core.db.pool(),
        &invoice.id,
        payment_method_id,
        core.clock.now(),
    )
    .await?;

    let charge = core
        .stripe
        .create_payment_intent(
            &tenant.stripe_customer_id,
            &intent.payment_method_id,
            &intent.id,
            &invoice.id,
            invoice.amount,
        )
        .await;
    let payment_intent_id = match charge {
        Ok(Charge::Succeeded(id)) => id,
        // Nothing was charged, so the next pass makes a new attempt on the tenant's current card.
        Ok(Charge::Refused(reason)) => {
            caravel_billing::tenants::commands::billing::delete_intent(core.db.pool(), &intent.id)
                .await?;
            caravel_billing::tenants::commands::set_tenant_stripe_error(
                core.db.pool(),
                &tenant.pubkey,
                Some(&reason),
            )
            .await?;
            return Err(Error::internal(reason));
        }
        // It may have gone through, so the attempt stays for the next pass to replay.
        Err(error) => {
            caravel_billing::tenants::commands::set_tenant_stripe_error(
                core.db.pool(),
                &tenant.pubkey,
                Some(&error.to_string()),
            )
            .await?;
            return Err(error.into());
        }
    };

    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            caravel_billing::tenants::commands::set_tenant_stripe_error(
                &mut *conn,
                &tenant.pubkey,
                None,
            )
            .await?;
            caravel_billing::tenants::commands::billing::mark_intent_settled(
                &mut *conn,
                &intent.id,
                &payment_intent_id,
                now,
            )
            .await?;
            caravel_billing::tenants::commands::billing::mark_invoice_paid(
                &mut *conn,
                &invoice.id,
                InvoiceMethod::Stripe,
                now,
            )
            .await
        })
        .await
}

/// DM the tenant a link to pay the invoice by hand, once it has had a day to be
/// seen in-app and at most every twelve days.
pub(super) async fn send_payment_reminder(
    core: &Core,
    tenant: &Tenant,
    invoice: &Invoice,
) -> Result<()> {
    let now = core.clock.now();

    if now - invoice.created_at < FRESH_INVOICE_DM_GRACE_SECS {
        return Ok(());
    }

    if invoice
        .notified_at
        .is_some_and(|at| now - at < MANUAL_PAYMENT_DM_INTERVAL_SECS)
    {
        return Ok(());
    }

    let payment_url = format!("{}?invoice={}", tenant.return_url, invoice.id);
    let message = format!("{MANUAL_PAYMENT_DM}\n\n{payment_url}");
    core.robot.send_dm(&tenant.pubkey, &message, now).await?;

    caravel_billing::tenants::commands::billing::mark_invoice_notified(
        core.db.pool(),
        &invoice.id,
        now,
    )
    .await
}

/// Expire an invoice's pending Checkout sessions once it is paid, so the tenant
/// can't pay twice. Lightning invoices can't be withdrawn.
pub(super) async fn cleanup_pending_payments(core: &Core, invoice: &Invoice) -> Result<()> {
    for checkout in caravel_billing::tenants::queries::billing::list_pending_checkouts_for_invoice(
        core.db.pool(),
        &invoice.id,
    )
    .await?
    {
        if let Err(error) = core
            .stripe
            .expire_checkout_session(&checkout.session_id)
            .await
        {
            tracing::debug!(invoice = %invoice.id, checkout = %checkout.id, error = %error, "could not expire checkout session");
        }
    }

    Ok(())
}

/// A payable Lightning invoice for an open invoice, reusing an unexpired unsettled
/// one. A paid or voided invoice gets its existing Lightning invoice, if any, and
/// never a new one. An expired one is only replaced once the platform's wallet says
/// it went unpaid; if it was paid, the invoice is settled and it is returned.
pub async fn ensure_bolt11_for_invoice(core: &Core, invoice: &Invoice) -> Result<Bolt11> {
    let now = core.clock.now();
    let existing = caravel_billing::tenants::queries::billing::get_bolt11_for_invoice(
        core.db.pool(),
        &invoice.id,
    )
    .await?;

    if !invoice.is_open() {
        return existing.ok_or_else(|| Error::conflict("invoice-not-open", "invoice is not open"));
    }

    if let Some(existing) = existing {
        // A settled one means the invoice was paid since the caller read it.
        if existing.settled_at.is_some() || now < existing.expires_at {
            return Ok(existing);
        }

        // Only the latest is ever looked up again, so a payment on this one would be lost.
        if core.wallet.is_settled(&existing.lnbc, now).await? {
            core.db
                .with_tx(async |conn| {
                    caravel_billing::tenants::commands::billing::mark_bolt11_settled(
                        &mut *conn,
                        &existing.id,
                        now,
                    )
                    .await?;
                    caravel_billing::tenants::commands::billing::mark_invoice_paid(
                        &mut *conn,
                        &invoice.id,
                        InvoiceMethod::Oob,
                        now,
                    )
                    .await
                })
                .await?;
            cleanup_pending_payments(core, invoice).await?;
            return Ok(Bolt11 {
                settled_at: Some(now),
                ..existing
            });
        }
    }

    let expiry: i64 = 3600;
    let msats = bitcoin::usd_cents_to_msats(invoice.amount).await?;
    let lnbc = core
        .wallet
        .make_invoice(msats, "Relay subscription payment", expiry as u64, now)
        .await?;

    caravel_billing::tenants::commands::billing::insert_bolt11(
        core.db.pool(),
        &invoice.id,
        &lnbc,
        msats as i64,
        now + expiry,
        now,
    )
    .await
}

/// A hosted Checkout session for an open invoice, reusing a pending unexpired one
/// so repeated clicks land on one page. Stripe sends the tenant back to
/// `return_url` on success or cancel, where reconciling settles the payment.
pub async fn ensure_checkout_for_invoice(
    core: &Core,
    tenant: &Tenant,
    invoice: &Invoice,
    return_url: &str,
) -> Result<Checkout> {
    let now = core.clock.now();

    if !invoice.is_open() {
        return Err(Error::conflict("invoice-not-open", "invoice is not open"));
    }

    if let Some(existing) = caravel_billing::tenants::queries::billing::get_checkout_for_invoice(
        core.db.pool(),
        &invoice.id,
    )
    .await?
        && now < existing.expires_at
    {
        if existing.settled_at.is_some() {
            return Err(Error::conflict(
                "checkout-settled",
                "a checkout has already been settled for this invoice",
            ));
        }
        return Ok(existing);
    }

    let session = core
        .stripe
        .create_checkout_session(
            &tenant.stripe_customer_id,
            &invoice.id,
            invoice.amount,
            return_url,
            return_url,
        )
        .await?;

    caravel_billing::tenants::commands::billing::insert_checkout(
        core.db.pool(),
        &invoice.id,
        &session.id,
        &session.url,
        session.expires_at,
        now,
    )
    .await
}

/// Collect an invoice at the tenant's request if it is still open: make it payable
/// by Lightning, then settle or charge it, without a reminder.
pub async fn collect_invoice(core: &Core, tenant: &Tenant, invoice: &Invoice) -> Result<()> {
    if !invoice.is_open() {
        return Ok(());
    }
    ensure_bolt11_for_invoice(core, invoice).await?;
    reconcile_payments(core, tenant, invoice, Collection::Charge).await
}

/// A Stripe billing portal where the tenant manages their saved cards.
pub async fn open_billing_portal(
    core: &Core,
    tenant: &Tenant,
    return_url: Option<&str>,
) -> Result<String> {
    Ok(core
        .stripe
        .create_billing_portal_session(&tenant.stripe_customer_id, return_url)
        .await?)
}

/// Refresh the tenant's cached payment method from Stripe, returning it, or the
/// cached one if Stripe can't be read. A card a churned tenant saved brings them
/// back, as new activity does.
pub(super) async fn sync_stripe_customer(core: &Core, tenant: &Tenant) -> Option<String> {
    let result: Result<Option<String>> = async {
        let payment_method_id = core
            .stripe
            .get_saved_payment_method(&tenant.stripe_customer_id)
            .await?;

        if payment_method_id != tenant.stripe_payment_method_id {
            // Before the card is stored, so a failure here is retried on the next sync.
            if tenant.churned_at.is_some() && payment_method_id.is_some() {
                reactivate_tenant(core, &tenant.pubkey).await?;
            }
            caravel_billing::tenants::commands::set_tenant_stripe_payment_method(
                core.db.pool(),
                &tenant.pubkey,
                payment_method_id.as_deref(),
            )
            .await?;
        }

        Ok(payment_method_id)
    }
    .await;

    result.unwrap_or_else(|error| {
        tracing::error!(tenant = %tenant.pubkey, error = ?error, "failed to sync payment method");
        tenant.stripe_payment_method_id.clone()
    })
}
