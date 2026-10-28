//! Pausing the relays of a tenant who does not pay.

use caravel_billing::relays::ActivityType;
use caravel_billing::relays::RelayStatus;
use caravel_billing::tenants::Invoice;
use caravel_billing::tenants::Tenant;
use caravel_util::bus::Change;
use caravel_util::error::{Error, Result};

use crate::Core;

use super::GRACE_PERIOD_SECS;

const CHURN_DM: &str = "Your relay subscription is past due, so your relays have been paused. You can restore service any time by adding a payment method from your dashboard:";

// --- Churn ---

/// Churn a tenant whose oldest open invoice is past the grace period: pause their
/// relays, void what they owe, and DM them once. Returns whether the tenant is past
/// due, in which case collection is skipped.
pub(super) async fn maybe_churn_tenant(
    core: &Core,
    tenant: &Tenant,
    invoices: &[Invoice],
) -> Result<bool> {
    let Some(oldest) = invoices.first() else {
        return Ok(false);
    };

    let now = core.clock.now();
    if now - oldest.created_at < GRACE_PERIOD_SECS {
        return Ok(false);
    }

    if tenant.churned_at.is_none() && churn_tenant(core, &tenant.pubkey).await? {
        let message = format!("{CHURN_DM}\n\n{}", tenant.return_url);
        if let Err(error) = core.robot.send_dm(&tenant.pubkey, &message, now).await {
            tracing::error!(tenant = %tenant.pubkey, error = %error, "failed to send churn DM");
        }
    }

    Ok(true)
}

/// Set the tenant's churn marker, mark its active relays delinquent, and void its
/// unpaid invoices, atomically. Returns whether this call churned the tenant, which
/// a concurrent pass may have done first.
pub(super) async fn churn_tenant(core: &Core, tenant_pubkey: &str) -> Result<bool> {
    let now = core.clock.now();
    let changed = core
        .db
        .with_tx(async |conn| {
            let locked =
                caravel_billing::tenants::queries::lock_tenant(conn, tenant_pubkey).await?;
            if locked.churned_at.is_some() {
                return Ok(None);
            }
            caravel_billing::tenants::commands::set_tenant_churned_at(
                &mut *conn,
                tenant_pubkey,
                Some(now),
            )
            .await?;

            let mut changed = Vec::new();
            for relay in
                caravel_billing::relays::queries::list_relays_for_tenant(&mut *conn, tenant_pubkey)
                    .await?
            {
                if relay.is_served() {
                    caravel_billing::relays::commands::activity::record_status_change(
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

            caravel_billing::tenants::commands::billing::void_open_invoices_for_tenant(
                conn,
                tenant_pubkey,
                now,
            )
            .await?;
            Ok::<_, Error>(Some(changed))
        })
        .await?;

    let Some(changed) = changed else {
        return Ok(false);
    };
    for relay_id in changed {
        core.bus.publish(Change::RelayChanged { relay_id });
    }
    Ok(true)
}
