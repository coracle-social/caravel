//! Folding relay activity into line items: subscriptions, plan changes,
//! renewals and metered overage.

use caravel_billing::relays::RelayStatus;
use caravel_billing::relays::{Activity, ActivityType, GB, Plan, SECONDS_PER_HOUR, Snapshot};
use caravel_billing::tenants::BillingPeriod;
use caravel_billing::tenants::InvoiceItem;
use caravel_billing::tenants::Tenant;
use caravel_util::error::{Error, Result};

use crate::Core;
use crate::usage;

use super::churn::maybe_churn_tenant;
use super::collect::{reconcile_payments, sync_stripe_customer};
use super::{Collection, reactivate_tenant};

/// Sample usage, then reconcile and collect every tenant's subscription.
pub async fn reconcile_subscriptions(core: &Core) -> Result<()> {
    // Sample first, so a period closing on this tick rolls up a fresh reading.
    if let Err(error) = usage::sample_usage(core).await {
        tracing::error!(error = %error, "failed to sample usage");
    }

    let tenants = caravel_billing::tenants::queries::list_tenants(core.db.pool()).await?;
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
        caravel_billing::relays::queries::activity::list_billable_activity_for_tenant(
            core.db.pool(),
            &tenant.pubkey,
        )
        .await?;

    // A churned tenant with new activity is back, and restoring their relays is priced now.
    if tenant.churned_at.is_some() && !activities.is_empty() {
        activities.extend(reactivate_tenant(core, &tenant.pubkey).await?);
        tenant.churned_at = None;
    }

    for activity in activities {
        if tenant.billing_anchor.is_none() {
            let anchor = caravel_billing::tenants::commands::ensure_tenant_billing_anchor(
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
                caravel_billing::tenants::queries::lock_tenant(conn, &tenant.pubkey).await?;
                caravel_billing::tenants::commands::billing::insert_invoice_for_outstanding_items(
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

    let invoices = caravel_billing::tenants::queries::billing::list_open_invoices_for_tenant(
        core.db.pool(),
        &tenant.pubkey,
    )
    .await?;

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
pub(super) async fn reconcile_activity(
    core: &Core,
    tenant: &Tenant,
    activity: &Activity,
) -> Result<()> {
    let status = &activity.snapshot.status;
    let id = uuid::Uuid::new_v4().to_string();

    let item = match activity.activity_type {
        ActivityType::CreateRelay => {
            caravel_billing::tenants::prorated_item(id, tenant, activity, 1, "New relay created")?
        }
        ActivityType::ActivateRelay | ActivityType::UnmarkRelayDelinquent => {
            caravel_billing::tenants::prorated_item(id, tenant, activity, 1, "Relay reactivated")?
        }
        ActivityType::DeactivateRelay => {
            caravel_billing::tenants::prorated_item(id, tenant, activity, -1, "Relay deactivated")?
        }
        // Only an active relay's purge is credited; an inactive or delinquent one wasn't billed.
        ActivityType::PurgeRelay if *status == RelayStatus::Active => {
            caravel_billing::tenants::prorated_item(id, tenant, activity, -1, "Relay purged")?
        }
        ActivityType::UpdateRelay => plan_change_item(core, tenant, activity).await?,
        ActivityType::PurgeRelay | ActivityType::MarkRelayDelinquent => None,
    };

    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            if caravel_billing::relays::commands::activity::mark_activity_billed(
                &mut *conn,
                &activity.id,
                now,
            )
            .await?
                && let Some(item) = &item
            {
                caravel_billing::tenants::commands::billing::insert_invoice_item(&mut *conn, item)
                    .await?;
            }
            Ok::<_, Error>(())
        })
        .await
}

/// The prorated difference a plan change makes, between the plan this update
/// recorded and the plan the relay had just before it. Renewal charges the plan as
/// of the period boundary, so these deltas compose to the right total in any order.
/// A relay that isn't active isn't billed, so changing its plan costs nothing; its
/// reactivation charges the new plan.
pub(super) async fn plan_change_item(
    core: &Core,
    tenant: &Tenant,
    activity: &Activity,
) -> Result<Option<InvoiceItem>> {
    if activity.snapshot.status != RelayStatus::Active {
        return Ok(None);
    }

    let new_plan_id = &activity.snapshot.plan_id;
    let previous = caravel_billing::relays::queries::activity::get_previous_relay_activity(
        core.db.pool(),
        activity,
    )
    .await?
    .ok_or_else(|| Error::internal("no previous plan found for relay update activity"))?;
    let old_plan_id = &previous.snapshot.plan_id;

    if old_plan_id == new_plan_id {
        return Ok(None);
    }

    let new_plan = Plan::recorded(new_plan_id)?;
    let old_plan = Plan::recorded(old_plan_id)?;

    let period = tenant.activity_period(activity)?;
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
pub(super) async fn reconcile_renewal(
    core: &Core,
    tenant: &Tenant,
    period: &BillingPeriod,
) -> Result<()> {
    if tenant.renewed_at.is_some_and(|at| at >= period.start) {
        return Ok(());
    }

    let mut items = Vec::new();
    for relay in
        caravel_billing::relays::queries::list_relays_for_tenant(core.db.pool(), &tenant.pubkey)
            .await?
    {
        if relay.is_purged() {
            continue;
        }
        let Some(activity) =
            caravel_billing::relays::queries::activity::get_latest_relay_activity_before(
                core.db.pool(),
                &relay.id,
                period.start,
            )
            .await?
        else {
            continue;
        };
        let Snapshot { plan_id, status } = &*activity.snapshot;
        if *status != RelayStatus::Active {
            continue;
        }
        let Some(plan) = caravel_billing::relays::plans::get_plan(plan_id).filter(Plan::is_paid)
        else {
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
            let locked =
                caravel_billing::tenants::queries::lock_tenant(conn, &tenant.pubkey).await?;
            if locked.renewed_at.is_some_and(|at| at >= period.start) {
                return Ok(());
            }
            for item in &items {
                caravel_billing::tenants::commands::billing::insert_invoice_item(&mut *conn, item)
                    .await?;
            }
            caravel_billing::tenants::commands::set_tenant_renewed_at(
                &mut *conn,
                &tenant.pubkey,
                period.start,
            )
            .await
        })
        .await
}

/// Bill the metered usage of the period before `period`, in arrears. Each relay is
/// priced at the plan its last activity before the close recorded, so an upgrade
/// in the new period doesn't reprice the old one. Runs once per closed period: the
/// tenant's `overage_billed_at` is checked and advanced under its row lock, even
/// when the period ran inside its allowance.
pub(super) async fn reconcile_overage(
    core: &Core,
    tenant: &Tenant,
    period: &BillingPeriod,
) -> Result<()> {
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

    let totals = caravel_billing::relays::queries::usage::get_usage_totals_for_tenant(
        core.db.pool(),
        &tenant.pubkey,
        previous.start,
        previous.end,
    )
    .await?;

    let mut items = Vec::new();
    for total in totals {
        let Some(activity) =
            caravel_billing::relays::queries::activity::get_latest_relay_activity_before(
                core.db.pool(),
                &total.relay_id,
                previous.end,
            )
            .await?
        else {
            continue;
        };
        let plan_id = &activity.snapshot.plan_id;
        let Some(plan) = caravel_billing::relays::plans::get_plan(plan_id).filter(Plan::is_paid)
        else {
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
            let locked =
                caravel_billing::tenants::queries::lock_tenant(conn, &tenant.pubkey).await?;
            if locked
                .overage_billed_at
                .is_some_and(|at| at >= previous.start)
            {
                return Ok(());
            }
            for item in &items {
                caravel_billing::tenants::commands::billing::insert_invoice_item(&mut *conn, item)
                    .await?;
            }
            caravel_billing::tenants::commands::set_tenant_overage_billed_at(
                &mut *conn,
                &tenant.pubkey,
                previous.start,
            )
            .await
        })
        .await
}

/// Bring a tenant's billing up to date at their request without charging them,
/// picking up a card they just saved and pricing the return it brings a churned
/// tenant.
pub async fn reconcile_tenant(core: &Core, tenant: &Tenant) -> Result<()> {
    sync_stripe_customer(core, tenant).await;
    let tenant =
        caravel_billing::tenants::queries::require_tenant(core.db.pool(), &tenant.pubkey).await?;
    reconcile_subscription(core, &tenant, Collection::Settle).await
}
