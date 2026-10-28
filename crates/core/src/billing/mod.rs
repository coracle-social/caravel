//! Subscription billing: reconciling each tenant's subscription, collecting what
//! it owes, and pausing the relays of those who do not pay.

pub mod churn;
pub mod collect;
pub mod invoices;
pub mod reconcile;

pub use collect::*;
pub use invoices::*;
pub use reconcile::*;

use caravel_billing::relays::{Activity, ActivityType, RelayStatus};
use caravel_util::bus::Change;
use caravel_util::error::{Error, Result};

use crate::Core;

/// How long a tenant has to pay an invoice before their relays are paused.
const GRACE_PERIOD_SECS: i64 = 7 * 24 * 60 * 60;

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

/// Clear the tenant's churn marker, restore its delinquent relays, and void any
/// still-open invoices, atomically. Returns the activity restoring each relay,
/// which the caller or the next reconcile prices, and none if the tenant isn't
/// churned, such as when a concurrent pass already brought them back.
pub(crate) async fn reactivate_tenant(core: &Core, tenant_pubkey: &str) -> Result<Vec<Activity>> {
    let now = core.clock.now();
    let activities = core
        .db
        .with_tx(async |conn| {
            let locked =
                caravel_billing::tenants::queries::lock_tenant(conn, tenant_pubkey).await?;
            if locked.churned_at.is_none() {
                return Ok(Vec::new());
            }
            caravel_billing::tenants::commands::set_tenant_churned_at(
                &mut *conn,
                tenant_pubkey,
                None,
            )
            .await?;

            let mut activities = Vec::new();
            for relay in
                caravel_billing::relays::queries::list_relays_for_tenant(&mut *conn, tenant_pubkey)
                    .await?
            {
                if relay.status == RelayStatus::Delinquent && !relay.is_purged() {
                    let activity =
                        caravel_billing::relays::commands::activity::record_status_change(
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

            caravel_billing::tenants::commands::billing::void_open_invoices_for_tenant(
                conn,
                tenant_pubkey,
                now,
            )
            .await?;
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
