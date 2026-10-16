//! Custom domains: validating the ones tenants choose, and verifying over DNS that
//! each points at its relay before the relay answers on it.

use std::sync::LazyLock;

use caravel_adapters::dns;
use regex::Regex;

use crate::Core;
use crate::bus::Change;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::relays;

static CUSTOM_DOMAIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([a-z0-9][a-z0-9\-]*\.)+[a-z]{2,}$").unwrap());

/// Lowercase and check a custom domain, where empty means none. A domain under
/// one of the relay domains belongs in the subdomain instead.
pub fn normalize_custom_domain(config: &Config, domain: &str) -> Result<Option<String>> {
    let domain = domain.trim().trim_end_matches('.').to_lowercase();

    if domain.is_empty() {
        return Ok(None);
    }

    if !CUSTOM_DOMAIN_RE.is_match(&domain) {
        return Err(Error::invalid(
            "invalid-domain",
            "domain must be a valid hostname (e.g. relay.example.com)",
        ));
    }

    if config.is_relay_domain(&domain) {
        return Err(Error::invalid(
            "reserved-domain",
            "use the subdomain field for domains under one of the platform's relay domains",
        ));
    }

    Ok(Some(domain))
}

/// Whether the platform serves `host`, which on-demand TLS asks before issuing a
/// certificate for it.
pub async fn is_served_host(core: &Core, host: &str) -> Result<bool> {
    let host = host.trim().trim_end_matches('.').to_lowercase();

    Ok(relays::get_relay_by_host(core.db.pool(), &host)
        .await?
        .is_some())
}

/// Verify each unverified custom domain that now points at its relay.
pub async fn verify_custom_domains(core: &Core) -> Result<()> {
    for relay in relays::list_relays_with_custom_domain(core.db.pool(), false).await? {
        let Some(domain) = relay.custom_domain.as_deref() else {
            continue;
        };
        match dns::domain_points_to(domain, &relay.canonical_host()).await {
            Ok(true) => {
                tracing::info!(relay = %relay.id, domain, "custom domain verified");
                set_verified(core, &relay.id, domain, true).await;
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
    for relay in relays::list_relays_with_custom_domain(core.db.pool(), true).await? {
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

async fn set_verified(core: &Core, relay_id: &str, custom_domain: &str, verified: bool) {
    let result: Result<bool> = core
        .db
        .with_tx(async |conn| {
            let changed =
                relays::set_custom_domain_verified(&mut *conn, relay_id, custom_domain, verified)
                    .await?;
            if changed && verified {
                relays::remove_host_redirects(&mut *conn, &[custom_domain.to_string()]).await?;
            }
            Ok(changed)
        })
        .await;

    match result {
        Ok(true) => core.bus.publish(Change::RelayChanged {
            relay_id: relay_id.to_string(),
        }),
        Ok(false) => {}
        Err(error) => {
            tracing::error!(relay = relay_id, error = %error, "failed to record custom domain verification")
        }
    }
}
