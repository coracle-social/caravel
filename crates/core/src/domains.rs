//! Custom domains: verifying over DNS that each points at its relay before the
//! relay answers on it.

use caravel_util::dns;

use crate::Core;
use caravel_db::{commands, queries};
use caravel_util::bus::Change;
use caravel_util::error::Result;

/// Whether the platform serves `host`, which on-demand TLS asks before issuing a
/// certificate for it.
pub async fn is_served_host(core: &Core, host: &str) -> Result<bool> {
    let host = host.trim().trim_end_matches('.').to_lowercase();

    Ok(queries::relays::get_relay_by_host(core.db.pool(), &host)
        .await?
        .is_some())
}

/// Verify each unverified custom domain that now points at its relay.
pub async fn verify_custom_domains(core: &Core) -> Result<()> {
    for relay in queries::relays::list_relays_with_custom_domain(core.db.pool(), false).await? {
        let Some(domain) = relay.custom_domain.as_deref() else {
            continue;
        };
        match dns::domain_points_to(domain, &relay.canonical_host()).await {
            Ok(true) => {
                if set_verified(core, &relay.id, domain, true).await {
                    tracing::info!(relay = %relay.id, domain, "custom domain verified");
                }
            }
            Ok(false) => {
                tracing::debug!(relay = %relay.id, domain, "custom domain not pointing at relay yet");
            }
            Err(error) => {
                tracing::warn!(relay = %relay.id, error = %error, "DNS check failed for custom domain");
            }
        }
    }
    Ok(())
}

/// Withdraw verification from each verified custom domain that no longer points at
/// its relay, which then answers on its canonical host again.
pub async fn reverify_custom_domains(core: &Core) -> Result<()> {
    for relay in queries::relays::list_relays_with_custom_domain(core.db.pool(), true).await? {
        let Some(domain) = relay.custom_domain.as_deref() else {
            continue;
        };
        match dns::domain_points_to(domain, &relay.canonical_host()).await {
            Ok(true) => {}
            Ok(false) => {
                tracing::warn!(relay = %relay.id, domain, "custom domain no longer points at relay");
                set_verified(core, &relay.id, domain, false).await;
            }
            Err(error) => {
                tracing::warn!(relay = %relay.id, error = %error, "DNS check failed for verified custom domain");
            }
        }
    }
    Ok(())
}

/// Record a verification result, returning whether it changed the relay. A
/// verified domain stops redirecting to whichever relay held it before, and both
/// relays reload.
async fn set_verified(core: &Core, relay_id: &str, custom_domain: &str, verified: bool) -> bool {
    let result: Result<(bool, Option<String>)> = core
        .db
        .with_tx(async |conn| {
            let changed = commands::relays::set_custom_domain_verified(
                &mut *conn,
                relay_id,
                custom_domain,
                verified,
            )
            .await?;
            let previous = if changed && verified {
                commands::relays::take_host_redirect(&mut *conn, custom_domain).await?
            } else {
                None
            };
            Ok((changed, previous))
        })
        .await;

    match result {
        Ok((true, previous)) => {
            let previous = previous.filter(|previous| previous != relay_id);
            for relay_id in std::iter::once(relay_id).chain(previous.as_deref()) {
                core.bus.publish(Change::RelayChanged {
                    relay_id: relay_id.to_string(),
                });
            }
            true
        }
        Ok((false, _)) => false,
        Err(error) => {
            tracing::error!(relay = relay_id, error = %error, "failed to record custom domain verification");
            false
        }
    }
}
