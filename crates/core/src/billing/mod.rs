//! Subscription billing: reconciling relay activity into line items, renewing
//! each period, billing metered overage, cutting invoices, and collecting them
//! (Lightning, then a card on file, then a reminder DM), churning tenants who
//! don't pay.

pub mod ledger;
pub mod period;

use caravel_adapters::{bitcoin, wallet::Wallet};

use crate::Core;
use crate::bus::Change;
use crate::error::{Error, Result};
use crate::plans::{self, GB, Plan, SECONDS_PER_HOUR};
use crate::relays::activity::{self, Activity, ActivityType, Snapshot};
use crate::relays::{self, RelayStatus};
use crate::tenants::{self, Tenant};
use ledger::{Bolt11, Checkout, Invoice, InvoiceItem, InvoiceMethod};
use period::BillingPeriod;

const GRACE_PERIOD_SECS: i64 = 7 * 24 * 60 * 60;

/// Hold the manual-payment DM until an open invoice is at least this old, since a
/// fresh invoice is shown to the tenant in-app first.
const FRESH_INVOICE_DM_GRACE_SECS: i64 = 24 * 60 * 60;
const MANUAL_PAYMENT_DM_INTERVAL_SECS: i64 = 12 * 24 * 60 * 60;
const MANUAL_PAYMENT_DM: &str = "Payment is due for your relay subscription. Open the link below to review the invoice and pay by Lightning or card:";
const CHURN_DM: &str = "Your relay subscription is past due, so your relays have been paused. You can restore service any time by adding a payment method or paying an invoice from your dashboard:";

/// How far collecting an open invoice goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Collection {
    /// Only settle what the tenant already paid out of band.
    Settle,
    /// Also charge the tenant's wallet, then their saved card.
    Charge,
    /// Also send a payment reminder when neither charge goes through.
    ChargeAndRemind,
}

/// Sample usage, then reconcile and collect every tenant's subscription.
pub async fn reconcile_subscriptions(core: &Core) -> Result<()> {
    // Sample first, so a period closing on this tick rolls up a fresh reading.
    if let Err(error) = crate::usage::sample_usage(core).await {
        tracing::error!(error = %error, "failed to sample usage");
    }

    let tenants = tenants::list_tenants(core.db.pool()).await?;
    tracing::info!(
        tenant_count = tenants.len(),
        "reconciling all subscriptions"
    );

    for tenant in tenants {
        if let Err(error) = reconcile_subscription(core, &tenant, Collection::ChargeAndRemind).await
        {
            tracing::error!(tenant = %tenant.pubkey, error = ?error, "failed to reconcile subscription");
        }
    }

    Ok(())
}

/// Bring a tenant's billing up to date: re-activate them if they churned and came
/// back, fold billable activity into line items, bill the closed period's overage,
/// renew the current period, claim outstanding items onto an invoice, churn them
/// if an invoice is past due, and otherwise collect each open invoice as far as
/// `collection` goes.
pub async fn reconcile_subscription(
    core: &Core,
    tenant: &Tenant,
    collection: Collection,
) -> Result<()> {
    // A ban forgave the tenant's charges and purged its relays, so nothing is owed.
    if tenant.banned_at.is_some() {
        return Ok(());
    }

    let mut tenant = tenant.clone();
    let mut activities =
        activity::list_billable_activity_for_tenant(core.db.pool(), &tenant.pubkey).await?;

    // A churned tenant with new activity is back, and restoring their relays is priced now.
    if tenant.churned_at.is_some() && !activities.is_empty() {
        activities.extend(reactivate_tenant(core, &tenant.pubkey).await?);
        tenant.churned_at = None;
    }

    for activity in activities {
        if tenant.billing_anchor.is_none() {
            let anchor = tenants::ensure_tenant_billing_anchor(
                core.db.pool(),
                &tenant.pubkey,
                activity.created_at,
            )
            .await?;
            tenant.billing_anchor = Some(anchor);
        }

        reconcile_activity(core, &tenant, &activity).await?;
    }

    // Admins' invoices are cut, so the ledger shows their usage, but issued paid.
    let now = core.clock.now();
    if let Some(period) = BillingPeriod::at(tenant.billing_anchor, now) {
        // The closed period's overage lands on the same invoice as the renewal.
        reconcile_overage(core, &tenant, &period).await?;
        reconcile_renewal(core, &tenant, &period).await?;

        let comped = core.config.is_admin(&tenant.pubkey);
        core.db
            .with_tx(async |conn| {
                tenants::lock_tenant(conn, &tenant.pubkey).await?;
                ledger::insert_invoice_for_outstanding_items(
                    conn,
                    &tenant.pubkey,
                    &period,
                    comped,
                    now,
                )
                .await
            })
            .await?;
    }

    let invoices = ledger::list_open_invoices_for_tenant(core.db.pool(), &tenant.pubkey).await?;

    if maybe_churn_tenant(core, &tenant, &invoices).await? {
        return Ok(());
    }

    if collection != Collection::Settle {
        tenant.stripe_payment_method_id = sync_stripe_customer(core, &tenant).await;
    }

    for invoice in &invoices {
        reconcile_payments(core, &tenant, invoice, collection).await?;
    }

    Ok(())
}

/// Price one activity and claim it, in one transaction so it is billed once.
/// Activity that costs nothing, such as a change on a free plan, is claimed too.
async fn reconcile_activity(core: &Core, tenant: &Tenant, activity: &Activity) -> Result<()> {
    let status = &activity.snapshot.status;

    let item = match activity.activity_type {
        ActivityType::CreateRelay => prorated_item(tenant, activity, 1, "New relay created")?,
        ActivityType::ActivateRelay | ActivityType::UnmarkRelayDelinquent => {
            prorated_item(tenant, activity, 1, "Relay reactivated")?
        }
        ActivityType::DeactivateRelay => prorated_item(tenant, activity, -1, "Relay deactivated")?,
        // Only an active relay's purge is credited; an inactive or delinquent one wasn't billed.
        ActivityType::PurgeRelay if *status == RelayStatus::Active => {
            prorated_item(tenant, activity, -1, "Relay purged")?
        }
        ActivityType::UpdateRelay => plan_change_item(core, tenant, activity).await?,
        ActivityType::PurgeRelay | ActivityType::MarkRelayDelinquent => None,
    };

    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            if activity::mark_activity_billed(&mut *conn, &activity.id, now).await?
                && let Some(item) = &item
            {
                ledger::insert_invoice_item(&mut *conn, item).await?;
            }
            Ok::<_, Error>(())
        })
        .await
}

/// A prorated charge, or a credit with `sign` -1, for the plan on the activity's
/// snapshot over what remains of its period. `None` for a free plan.
fn prorated_item(
    tenant: &Tenant,
    activity: &Activity,
    sign: i64,
    description: &str,
) -> Result<Option<InvoiceItem>> {
    let plan_id = &activity.snapshot.plan_id;
    let plan = recorded_plan(plan_id)?;
    if !plan.is_paid() {
        return Ok(None);
    }

    let period = activity_period(tenant, activity)?;

    Ok(Some(InvoiceItem {
        id: uuid::Uuid::new_v4().to_string(),
        invoice_id: None,
        activity_id: Some(activity.id.clone()),
        tenant_pubkey: activity.tenant_pubkey.clone(),
        relay_id: activity.relay_id.clone(),
        plan_id: plan.id,
        amount: sign * period.prorate(plan.amount, activity.created_at),
        description: description.to_string(),
        created_at: activity.created_at,
        voided_at: None,
    }))
}

/// The plan an activity recorded, which billing can't price if the catalog lost it.
fn recorded_plan(id: &str) -> Result<Plan> {
    plans::get_plan(id).ok_or_else(|| Error::internal(format!("unknown plan {id}")))
}

/// The period an activity fell in, by the tenant's billing anchor.
fn activity_period(tenant: &Tenant, activity: &Activity) -> Result<BillingPeriod> {
    BillingPeriod::at(tenant.billing_anchor, activity.created_at)
        .ok_or_else(|| Error::internal("billing anchor must be set before pricing activity"))
}

/// The prorated difference a plan change makes, between the plan this update
/// recorded and the plan the relay had just before it. Renewal charges the plan as
/// of the period boundary, so these deltas compose to the right total in any order.
async fn plan_change_item(
    core: &Core,
    tenant: &Tenant,
    activity: &Activity,
) -> Result<Option<InvoiceItem>> {
    let new_plan_id = &activity.snapshot.plan_id;
    let previous = activity::get_previous_relay_activity(core.db.pool(), activity)
        .await?
        .ok_or_else(|| Error::internal("no previous plan found for relay update activity"))?;
    let old_plan_id = &previous.snapshot.plan_id;

    if old_plan_id == new_plan_id {
        return Ok(None);
    }

    let new_plan = recorded_plan(new_plan_id)?;
    let old_plan = recorded_plan(old_plan_id)?;

    let period = activity_period(tenant, activity)?;
    let amount = period.prorate(new_plan.amount, activity.created_at)
        - period.prorate(old_plan.amount, activity.created_at);
    if amount == 0 {
        return Ok(None);
    }

    Ok(Some(InvoiceItem {
        id: uuid::Uuid::new_v4().to_string(),
        invoice_id: None,
        activity_id: Some(activity.id.clone()),
        tenant_pubkey: activity.tenant_pubkey.clone(),
        relay_id: activity.relay_id.clone(),
        plan_id: new_plan.id,
        amount,
        description: format!("Plan changed from {} to {}", old_plan.name, new_plan.name),
        created_at: activity.created_at,
        voided_at: None,
    }))
}

/// Charge a full period for every relay that was active on a paid plan as of
/// `period.start`, read from each relay's last activity before the boundary. A
/// relay created within the period is covered by its own prorated charge. Runs once
/// per period: the tenant's `renewed_at` is checked and advanced under its row lock.
async fn reconcile_renewal(core: &Core, tenant: &Tenant, period: &BillingPeriod) -> Result<()> {
    if tenant.renewed_at.is_some_and(|at| at >= period.start) {
        return Ok(());
    }

    let mut items = Vec::new();
    for relay in relays::list_relays_for_tenant(core.db.pool(), &tenant.pubkey).await? {
        if relay.is_purged() {
            continue;
        }
        let Some(activity) =
            activity::get_latest_relay_activity_before(core.db.pool(), &relay.id, period.start)
                .await?
        else {
            continue;
        };
        let Snapshot { plan_id, status } = &*activity.snapshot;
        if *status != RelayStatus::Active {
            continue;
        }
        let Some(plan) = plans::get_plan(plan_id).filter(Plan::is_paid) else {
            continue;
        };

        items.push(InvoiceItem {
            id: uuid::Uuid::new_v4().to_string(),
            invoice_id: None,
            activity_id: None,
            tenant_pubkey: tenant.pubkey.clone(),
            relay_id: relay.id,
            plan_id: plan.id,
            amount: plan.amount,
            description: "Subscription renewal".to_string(),
            created_at: period.start,
            voided_at: None,
        });
    }

    // The marker stays, so a relay renewable later this period is renewed at the next boundary.
    if items.is_empty() {
        return Ok(());
    }

    core.db
        .with_tx(async |conn| {
            let locked = tenants::lock_tenant(conn, &tenant.pubkey).await?;
            if locked.renewed_at.is_some_and(|at| at >= period.start) {
                return Ok(());
            }
            for item in &items {
                ledger::insert_invoice_item(&mut *conn, item).await?;
            }
            tenants::set_tenant_renewed_at(&mut *conn, &tenant.pubkey, period.start).await
        })
        .await
}

/// Bill the metered usage of the period before `period`, in arrears. Each relay is
/// priced at the plan its last activity before the close recorded, so an upgrade
/// in the new period doesn't reprice the old one. Runs once per closed period: the
/// tenant's `overage_billed_at` is checked and advanced under its row lock, even
/// when the period ran inside its allowance.
async fn reconcile_overage(core: &Core, tenant: &Tenant, period: &BillingPeriod) -> Result<()> {
    let Some(previous) = BillingPeriod::at(tenant.billing_anchor, period.start - 1) else {
        return Ok(());
    };

    // The tenant's first period has nothing before it to settle.
    if previous.start >= period.start
        || tenant
            .overage_billed_at
            .is_some_and(|at| at >= previous.start)
    {
        return Ok(());
    }

    let totals = crate::usage::get_usage_totals_for_tenant(
        core.db.pool(),
        &tenant.pubkey,
        previous.start,
        previous.end,
    )
    .await?;

    let mut items = Vec::new();
    for total in totals {
        let Some(activity) = activity::get_latest_relay_activity_before(
            core.db.pool(),
            &total.relay_id,
            previous.end,
        )
        .await?
        else {
            continue;
        };
        let plan_id = &activity.snapshot.plan_id;
        let Some(plan) = plans::get_plan(plan_id).filter(Plan::is_paid) else {
            continue;
        };

        // Rounding up before the allowance comes off leaves an allowance used exactly free.
        let used_gb = (total.peak_storage_bytes.max(0) as u64).div_ceil(GB as u64) as i64;
        let used_hours =
            (total.participant_seconds.max(0) as u64).div_ceil(SECONDS_PER_HOUR as u64) as i64;

        let meters = [
            (
                "Storage",
                "GB",
                used_gb,
                plan.storage_gb,
                plan.storage_overage,
            ),
            (
                "A/V call",
                "participant hours",
                used_hours,
                plan.av_hours,
                plan.av_overage,
            ),
        ];

        for (label, unit, used, included, rate) in meters {
            let over = (used - included).max(0);
            let amount = over * rate;
            if amount <= 0 {
                continue;
            }

            items.push(InvoiceItem {
                id: uuid::Uuid::new_v4().to_string(),
                invoice_id: None,
                activity_id: None,
                tenant_pubkey: tenant.pubkey.clone(),
                relay_id: total.relay_id.clone(),
                plan_id: plan.id.clone(),
                amount,
                description: format!(
                    "{label} overage ({over} {unit} past the {included} included)"
                ),
                created_at: previous.end,
                voided_at: None,
            });
        }
    }

    core.db
        .with_tx(async |conn| {
            let locked = tenants::lock_tenant(conn, &tenant.pubkey).await?;
            if locked
                .overage_billed_at
                .is_some_and(|at| at >= previous.start)
            {
                return Ok(());
            }
            for item in &items {
                ledger::insert_invoice_item(&mut *conn, item).await?;
            }
            tenants::set_tenant_overage_billed_at(&mut *conn, &tenant.pubkey, previous.start).await
        })
        .await
}

// --- Churn ---

/// Churn a tenant whose oldest open invoice is past the grace period: pause their
/// relays, void what they owe, and DM them once. Returns whether the tenant is past
/// due, in which case collection is skipped.
async fn maybe_churn_tenant(core: &Core, tenant: &Tenant, invoices: &[Invoice]) -> Result<bool> {
    let Some(oldest) = invoices.first() else {
        return Ok(false);
    };

    let now = core.clock.now();
    if now - oldest.created_at < GRACE_PERIOD_SECS {
        return Ok(false);
    }

    if tenant.churned_at.is_none() {
        churn_tenant(core, &tenant.pubkey).await?;

        let message = format!("{CHURN_DM}\n\n{}", tenant.return_url);
        if let Err(error) = core.robot.send_dm(&tenant.pubkey, &message, now).await {
            tracing::error!(tenant = %tenant.pubkey, error = %error, "failed to send churn DM");
        }
    }

    Ok(true)
}

/// Set the tenant's churn marker, mark its active relays delinquent, and void its
/// unpaid invoices, atomically.
async fn churn_tenant(core: &Core, tenant_pubkey: &str) -> Result<()> {
    let now = core.clock.now();
    let changed = core
        .db
        .with_tx(async |conn| {
            tenants::lock_tenant(conn, tenant_pubkey).await?;
            tenants::set_tenant_churned_at(&mut *conn, tenant_pubkey, Some(now)).await?;

            let mut changed = Vec::new();
            for relay in relays::list_relays_for_tenant(&mut *conn, tenant_pubkey).await? {
                if relay.is_served() {
                    activity::record_status_change(
                        conn,
                        &relay,
                        RelayStatus::Delinquent,
                        ActivityType::MarkRelayDelinquent,
                        now,
                    )
                    .await?;
                    changed.push(relay.id);
                }
            }

            ledger::void_open_invoices_for_tenant(conn, tenant_pubkey, now).await?;
            Ok::<_, Error>(changed)
        })
        .await?;

    for relay_id in changed {
        core.bus.publish(Change::RelayChanged { relay_id });
    }
    Ok(())
}

/// Clear the tenant's churn marker, restore its delinquent relays, and void any
/// still-open invoices, atomically. Returns the activity restoring each relay, for
/// the caller to price.
async fn reactivate_tenant(core: &Core, tenant_pubkey: &str) -> Result<Vec<Activity>> {
    let now = core.clock.now();
    let activities = core
        .db
        .with_tx(async |conn| {
            tenants::lock_tenant(conn, tenant_pubkey).await?;
            tenants::set_tenant_churned_at(&mut *conn, tenant_pubkey, None).await?;

            let mut activities = Vec::new();
            for relay in relays::list_relays_for_tenant(&mut *conn, tenant_pubkey).await? {
                if relay.status == RelayStatus::Delinquent && !relay.is_purged() {
                    let activity = activity::record_status_change(
                        conn,
                        &relay,
                        RelayStatus::Active,
                        ActivityType::UnmarkRelayDelinquent,
                        now,
                    )
                    .await?;
                    activities.push(activity);
                }
            }

            ledger::void_open_invoices_for_tenant(conn, tenant_pubkey, now).await?;
            Ok::<_, Error>(activities)
        })
        .await?;

    for activity in &activities {
        core.bus.publish(Change::RelayChanged {
            relay_id: activity.relay_id.clone(),
        });
    }
    Ok(activities)
}

// --- Payments ---

/// Collect an invoice as far as `collection` goes. Payments that may already be in
/// flight, a Lightning invoice or a Checkout session the tenant paid, are settled
/// first, so a fresh charge is never made on top of one. A failed charge's error is
/// stored on the tenant but never stops the cascade.
async fn reconcile_payments(
    core: &Core,
    tenant: &Tenant,
    invoice: &Invoice,
    collection: Collection,
) -> Result<()> {
    let now = core.clock.now();

    if let Some(bolt11) = ledger::get_bolt11_for_invoice(core.db.pool(), &invoice.id).await?
        && bolt11.settled_at.is_none()
        && core
            .wallet
            .is_settled(&bolt11.lnbc, now)
            .await
            .unwrap_or(false)
    {
        core.db
            .with_tx(async |conn| {
                ledger::mark_bolt11_settled(&mut *conn, &bolt11.id, now).await?;
                ledger::mark_invoice_paid(&mut *conn, &invoice.id, InvoiceMethod::Oob, now).await
            })
            .await?;
        return cleanup_pending_payments(core, invoice).await;
    }

    if let Some(checkout) = ledger::get_checkout_for_invoice(core.db.pool(), &invoice.id).await?
        && checkout.settled_at.is_none()
        && core
            .stripe
            .is_checkout_paid(&checkout.session_id)
            .await
            .unwrap_or(false)
    {
        core.db
            .with_tx(async |conn| {
                tenants::set_tenant_stripe_error(&mut *conn, &tenant.pubkey, None).await?;
                ledger::mark_checkout_settled(&mut *conn, &checkout.id, now).await?;
                ledger::mark_invoice_paid(&mut *conn, &invoice.id, InvoiceMethod::Stripe, now).await
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
async fn pay_with_nwc(
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
            && !matches!(ledger::get_invoice(core.db.pool(), &invoice.id).await, Ok(Some(paid)) if paid.paid_at.is_some())
        {
            return Err(error.into());
        }

        core.db
            .with_tx(async |conn| {
                tenants::set_tenant_nwc_error(&mut *conn, &tenant.pubkey, None).await?;
                ledger::mark_bolt11_settled(&mut *conn, &bolt11.id, now).await?;
                ledger::mark_invoice_paid(&mut *conn, &invoice.id, InvoiceMethod::Nwc, now).await
            })
            .await
    }
    .await;

    if let Err(error) = &result {
        tenants::set_tenant_nwc_error(core.db.pool(), &tenant.pubkey, Some(&error.to_string()))
            .await?;
    }

    result
}

async fn pay_with_card(
    core: &Core,
    tenant: &Tenant,
    invoice: &Invoice,
    payment_method_id: &str,
) -> Result<()> {
    let intent = ledger::ensure_pending_intent(
        core.db.pool(),
        &invoice.id,
        payment_method_id,
        core.clock.now(),
    )
    .await?;

    let payment_intent_id = match core
        .stripe
        .create_payment_intent(
            &tenant.stripe_customer_id,
            &intent.payment_method_id,
            &invoice.id,
            invoice.amount,
        )
        .await
    {
        Ok(id) => id,
        // Drop the attempt so the next pass starts clean on the tenant's current method.
        Err(error) => {
            ledger::delete_intent(core.db.pool(), &intent.id).await?;
            tenants::set_tenant_stripe_error(
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
            tenants::set_tenant_stripe_error(&mut *conn, &tenant.pubkey, None).await?;
            ledger::mark_intent_settled(&mut *conn, &intent.id, &payment_intent_id, now).await?;
            ledger::mark_invoice_paid(&mut *conn, &invoice.id, InvoiceMethod::Stripe, now).await
        })
        .await
}

/// DM the tenant a link to pay the invoice by hand, once it has had a day to be
/// seen in-app and at most every twelve days.
async fn send_payment_reminder(core: &Core, tenant: &Tenant, invoice: &Invoice) -> Result<()> {
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

    ledger::mark_invoice_notified(core.db.pool(), &invoice.id, now).await
}

/// Expire an invoice's pending Checkout sessions once it is paid, so the tenant
/// can't pay twice. Lightning invoices can't be withdrawn.
async fn cleanup_pending_payments(core: &Core, invoice: &Invoice) -> Result<()> {
    for checkout in ledger::list_pending_checkouts_for_invoice(core.db.pool(), &invoice.id).await? {
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
/// never a new one.
pub async fn ensure_bolt11_for_invoice(core: &Core, invoice: &Invoice) -> Result<Bolt11> {
    let now = core.clock.now();
    let existing = ledger::get_bolt11_for_invoice(core.db.pool(), &invoice.id).await?;

    if !invoice.is_open() {
        return existing.ok_or_else(|| Error::conflict("invoice-not-open", "invoice is not open"));
    }

    if let Some(existing) = existing
        && existing.settled_at.is_none()
        && now < existing.expires_at
    {
        return Ok(existing);
    }

    let expiry: i64 = 3600;
    let msats = bitcoin::usd_cents_to_msats(invoice.amount).await?;
    let lnbc = core
        .wallet
        .make_invoice(msats, "Relay subscription payment", expiry as u64, now)
        .await?;

    ledger::insert_bolt11(
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

    if let Some(existing) = ledger::get_checkout_for_invoice(core.db.pool(), &invoice.id).await?
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

    ledger::insert_checkout(
        core.db.pool(),
        &invoice.id,
        &session.id,
        &session.url,
        session.expires_at,
        now,
    )
    .await
}

/// Bring a tenant's billing up to date at their request without charging them,
/// picking up a card they just saved.
pub async fn reconcile_tenant(core: &Core, tenant: &Tenant) -> Result<()> {
    sync_stripe_customer(core, tenant).await;
    let tenant = tenants::require_tenant(core.db.pool(), &tenant.pubkey).await?;
    reconcile_subscription(core, &tenant, Collection::Settle).await
}

/// Collect an open invoice at the tenant's request: make it payable by Lightning,
/// then settle or charge it, without a reminder.
pub async fn collect_invoice(core: &Core, tenant: &Tenant, invoice: &Invoice) -> Result<()> {
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
/// cached one if Stripe can't be read.
async fn sync_stripe_customer(core: &Core, tenant: &Tenant) -> Option<String> {
    let result: Result<Option<String>> = async {
        let payment_method_id = core
            .stripe
            .get_saved_payment_method(&tenant.stripe_customer_id)
            .await?;

        if payment_method_id != tenant.stripe_payment_method_id {
            tenants::set_tenant_stripe_payment_method(
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

/// The draft invoice: an unsaved invoice summing the tenant's outstanding items for
/// the current period, or `None` when nothing is outstanding.
pub async fn get_draft_invoice(core: &Core, tenant: &Tenant) -> Result<Option<Invoice>> {
    let now = core.clock.now();
    let Some(period) = BillingPeriod::at(tenant.billing_anchor, now) else {
        return Ok(None);
    };

    let items =
        ledger::list_outstanding_invoice_items_for_tenant(core.db.pool(), &tenant.pubkey).await?;
    if items.is_empty() {
        return Ok(None);
    }

    Ok(Some(Invoice {
        id: "draft".to_string(),
        tenant_pubkey: tenant.pubkey.clone(),
        amount: items.iter().map(|item| item.amount).sum(),
        period_start: period.start,
        period_end: period.end,
        created_at: now,
        paid_at: None,
        voided_at: None,
        notified_at: None,
        method: None,
    }))
}
