//! Hosting: the operations tenants and admins perform on tenants and relays, each
//! of which spans the relay row, its activity, billing and the relay's hosts.
//! Callers authorize before calling.

use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

use crate::Core;
use crate::billing::ledger;
use crate::bus::Change;
use crate::crypto::generate_keypair;
use crate::domains::normalize_custom_domain;
use crate::error::{Error, Result};
use crate::flag::Flag;
use crate::plans;
use crate::relays::activity::{self, ActivityType, Snapshot};
use crate::relays::{self, Relay, RelayStatus};
use crate::tenants::{self, Tenant};

const RESERVED_SUBDOMAINS: [&str; 3] = ["api", "admin", "internal"];

static SUBDOMAIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]{1,61}[a-z0-9]$").unwrap());

/// The settings a tenant chooses for a relay. Creating a relay fills the gaps with
/// defaults; updating one leaves them as they were.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RelaySettings {
    pub subdomain: Option<String>,
    #[serde(rename = "zooid_domain")]
    pub domain: Option<String>,
    pub plan_id: Option<String>,
    pub info_name: Option<String>,
    pub info_icon: Option<String>,
    pub info_description: Option<String>,
    pub policy_public_read: Option<Flag>,
    pub policy_public_write: Option<Flag>,
    pub policy_public_join: Option<Flag>,
    pub policy_strip_signatures: Option<Flag>,
    pub groups_enabled: Option<Flag>,
    pub management_enabled: Option<Flag>,
    pub members_can_invite: Option<Flag>,
    pub blossom_enabled: Option<Flag>,
    pub livekit_enabled: Option<Flag>,
    pub push_enabled: Option<Flag>,
    pub custom_domain: Option<String>,
}

// --- Tenants ---

/// The tenant for `pubkey`, created with a Stripe customer if it doesn't exist.
/// An existing tenant keeps the `return_url` it was created with.
pub async fn ensure_tenant(core: &Core, pubkey: &str, return_url: &str) -> Result<Tenant> {
    let return_url = return_url.trim().trim_end_matches('/');
    if return_url.is_empty() {
        return Err(Error::conflict(
            "return-url-required",
            "return_url is required",
        ));
    }

    if let Some(tenant) = tenants::get_tenant(core.db.pool(), pubkey).await? {
        return Ok(tenant);
    }

    let name = core
        .robot
        .fetch_nostr_name(pubkey)
        .await
        .unwrap_or_else(|| pubkey.chars().take(8).collect());
    let stripe_customer_id = core.stripe.create_customer(pubkey, &name).await?;

    let tenant = Tenant {
        pubkey: pubkey.to_string(),
        return_url: return_url.to_string(),
        created_at: core.clock.now(),
        stripe_customer_id,
        ..Default::default()
    };
    tenants::insert_tenant(core.db.pool(), &tenant).await?;

    tenants::get_tenant(core.db.pool(), pubkey)
        .await?
        .ok_or_else(|| Error::internal("tenant missing after insert"))
}

/// Replace the tenant's NWC url, stored encrypted. Empty removes it.
pub async fn set_tenant_nwc_url(core: &Core, pubkey: &str, nwc_url: &str) -> Result<()> {
    let encrypted = if nwc_url.is_empty() {
        String::new()
    } else {
        core.config.platform_key.encrypt(nwc_url)?
    };

    tenants::set_tenant_nwc_url(core.db.pool(), pubkey, &encrypted).await
}

/// Ban a tenant: block relay creation, forgive what they owe, and purge their
/// relays. Purges stay unbilled, so their credit is still owed if the tenant is
/// unbanned.
pub async fn ban_tenant(core: &Core, pubkey: &str) -> Result<()> {
    let now = core.clock.now();
    let purged = core
        .db
        .with_tx(async |conn| {
            let tenant = tenants::lock_tenant(conn, pubkey).await?;
            if tenant.banned_at.is_some() {
                return Err(Error::conflict(
                    "tenant-is-banned",
                    "tenant is already banned",
                ));
            }

            tenants::set_tenant_banned_at(&mut *conn, pubkey, Some(now)).await?;
            activity::forgive_unbilled_activity_for_tenant(&mut *conn, pubkey, now).await?;
            ledger::void_open_invoices_for_tenant(conn, pubkey, now).await?;

            let mut purged = Vec::new();
            for relay in relays::list_relays_for_tenant(&mut *conn, pubkey).await? {
                if !relay.is_purged() {
                    purge_relay_in_tx(conn, &relay, now).await?;
                    purged.push(relay.id);
                }
            }
            Ok(purged)
        })
        .await?;

    for relay_id in purged {
        core.bus.publish(Change::RelayChanged { relay_id });
    }
    Ok(())
}

/// Lift a tenant's ban. Purged relays stay purged and forgiven charges forgiven.
pub async fn unban_tenant(core: &Core, pubkey: &str) -> Result<()> {
    let tenant = tenants::get_tenant(core.db.pool(), pubkey)
        .await?
        .ok_or(Error::NotFound("tenant not found"))?;
    if tenant.banned_at.is_none() {
        return Err(Error::conflict("tenant-not-banned", "tenant is not banned"));
    }

    tenants::set_tenant_banned_at(core.db.pool(), pubkey, None).await
}

// --- Relays ---

/// Create a relay for a tenant, with a keypair of its own.
pub async fn create_relay(
    core: &Core,
    tenant_pubkey: &str,
    settings: RelaySettings,
) -> Result<Relay> {
    let tenant = tenants::get_tenant(core.db.pool(), tenant_pubkey)
        .await?
        .ok_or(Error::NotFound("tenant not found"))?;
    if tenant.banned_at.is_some() {
        return Err(Error::conflict("tenant-is-banned", "tenant is banned"));
    }

    let required = |value: &Option<String>, field: &str| {
        value
            .clone()
            .ok_or_else(|| Error::invalid("missing-field", format!("{field} is required")))
    };
    let subdomain = required(&settings.subdomain, "subdomain")?;
    let domain = required(&settings.domain, "zooid_domain")?;
    let plan_id = required(&settings.plan_id, "plan_id")?;

    let (pubkey, secret) = generate_keypair();
    let now = core.clock.now();
    let relay = Relay {
        id: format!(
            "{}_{}",
            subdomain.replace('-', "_"),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ),
        tenant_pubkey: tenant_pubkey.to_string(),
        subdomain,
        domain,
        plan_id,
        status: RelayStatus::Active,
        pubkey,
        secret: core.config.platform_key.encrypt(&secret)?,
        groups_enabled: true,
        management_enabled: true,
        members_can_invite: true,
        push_enabled: true,
        created_at: now,
        ..Default::default()
    };
    let relay = validate_relay(core, apply_settings(core, relay, settings)?)?;

    core.db
        .with_tx(async |conn| {
            relays::insert_relay(conn, &relay).await?;
            let snapshot = Snapshot::Relay {
                plan: relay.plan_id.clone(),
                status: relay.status,
            };
            activity::insert_relay_activity(
                conn,
                ActivityType::CreateRelay,
                &relay.id,
                snapshot,
                now,
            )
            .await
        })
        .await
        .map_err(map_relay_write_error)?;

    core.bus.publish(Change::RelayChanged {
        relay_id: relay.id.clone(),
    });

    relays::get_relay(core.db.pool(), &relay.id)
        .await?
        .ok_or_else(|| Error::internal("relay missing after insert"))
}

/// Change a relay's settings. Moving it to another domain or replacing its custom
/// domain withdraws the custom domain's verification; a replaced verified domain
/// redirects to the relay from then on. Downgrading to a plan with a member limit
/// the relay is over is refused.
pub async fn update_relay(core: &Core, relay: Relay, settings: RelaySettings) -> Result<Relay> {
    if relay.is_purged() {
        return Err(Error::conflict("relay-is-purged", "relay has been purged"));
    }

    let previous = relay.clone();
    let relay = validate_relay(core, apply_settings(core, relay, settings)?)?;

    if relay.plan_id != previous.plan_id
        && let Some(limit) =
            plans::get_plan(&relay.plan_id).and_then(|plan| plan.members.map(|limit| (plan, limit)))
    {
        let (plan, limit) = limit;
        let members = list_relay_members(core, &relay).await?.len() as i64;
        if members > limit {
            return Err(Error::invalid(
                "member-limit-exceeded",
                format!(
                    "relay has {members} members, which exceeds the {} plan limit of {limit}",
                    plan.name.to_lowercase()
                ),
            ));
        }
    }

    let now = core.clock.now();
    let replaced_domain = (previous.custom_domain_verified
        && !previous.custom_domain.is_empty()
        && previous.custom_domain != relay.custom_domain)
        .then(|| previous.custom_domain.clone());
    let own_hosts = vec![relay.canonical_host(), relay.custom_domain.clone()];

    core.db
        .with_tx(async |conn| {
            relays::update_relay(conn, &relay).await?;
            if let Some(host) = &replaced_domain {
                relays::add_host_redirect(&mut *conn, host, &relay.id, now).await?;
            }
            relays::remove_host_redirects(&mut *conn, &own_hosts).await?;
            let snapshot = Snapshot::Relay {
                plan: relay.plan_id.clone(),
                status: relay.status,
            };
            activity::insert_relay_activity(
                conn,
                ActivityType::UpdateRelay,
                &relay.id,
                snapshot,
                now,
            )
            .await
        })
        .await
        .map_err(map_relay_write_error)?;

    core.bus.publish(Change::RelayChanged {
        relay_id: relay.id.clone(),
    });

    relays::get_relay(core.db.pool(), &relay.id)
        .await?
        .ok_or(Error::NotFound("relay not found"))
}

/// Pause a relay at its tenant's request, crediting the rest of the period.
pub async fn deactivate_relay(core: &Core, relay: &Relay) -> Result<()> {
    match relay.status {
        _ if relay.is_purged() => Err(Error::conflict("relay-is-purged", "relay has been purged")),
        RelayStatus::Delinquent => Err(Error::conflict(
            "relay-is-delinquent",
            "relay is delinquent",
        )),
        RelayStatus::Inactive => Err(Error::conflict(
            "relay-is-inactive",
            "relay is already inactive",
        )),
        RelayStatus::Active => {
            change_relay_status(
                core,
                relay,
                RelayStatus::Inactive,
                ActivityType::DeactivateRelay,
            )
            .await
        }
    }
}

/// Resume a paused relay, charging the rest of the period.
pub async fn reactivate_relay(core: &Core, relay: &Relay) -> Result<()> {
    match relay.status {
        _ if relay.is_purged() => Err(Error::conflict("relay-is-purged", "relay has been purged")),
        RelayStatus::Delinquent => Err(Error::conflict(
            "relay-is-delinquent",
            "relay is delinquent",
        )),
        RelayStatus::Active => Err(Error::conflict(
            "relay-is-active",
            "relay is already active",
        )),
        RelayStatus::Inactive => {
            change_relay_status(
                core,
                relay,
                RelayStatus::Active,
                ActivityType::ActivateRelay,
            )
            .await
        }
    }
}

/// Purge a relay: delete its data and release its hosts for good. The row stays.
pub async fn purge_relay(core: &Core, relay: &Relay) -> Result<()> {
    if relay.is_purged() {
        return Err(Error::conflict(
            "relay-is-purged",
            "relay is already purged",
        ));
    }

    let now = core.clock.now();
    core.db
        .with_tx(async |conn| purge_relay_in_tx(conn, relay, now).await)
        .await?;

    core.bus.publish(Change::RelayChanged {
        relay_id: relay.id.clone(),
    });
    Ok(())
}

/// The pubkeys that are members of a relay.
pub async fn list_relay_members(core: &Core, relay: &Relay) -> Result<Vec<String>> {
    if !relay.synced {
        return Ok(Vec::new());
    }

    Ok(core.zooid.list_members(&relay.id).await?)
}

async fn change_relay_status(
    core: &Core,
    relay: &Relay,
    status: RelayStatus,
    activity_type: ActivityType,
) -> Result<()> {
    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            activity::record_status_change(conn, relay, status, activity_type, now).await
        })
        .await?;

    core.bus.publish(Change::RelayChanged {
        relay_id: relay.id.clone(),
    });
    Ok(())
}

/// Mark a relay purged and record it, keeping the status it was purged from on the
/// activity so billing knows whether to credit it.
async fn purge_relay_in_tx(conn: &mut sqlx::PgConnection, relay: &Relay, now: i64) -> Result<()> {
    relays::set_relay_purged_at(&mut *conn, &relay.id, now).await?;
    let snapshot = Snapshot::Relay {
        plan: relay.plan_id.clone(),
        status: relay.status,
    };
    activity::insert_relay_activity(conn, ActivityType::PurgeRelay, &relay.id, snapshot, now)
        .await?;
    Ok(())
}

/// Overlay chosen settings on a relay. Changing the domain or the custom domain
/// withdraws the custom domain's verification.
fn apply_settings(core: &Core, mut relay: Relay, settings: RelaySettings) -> Result<Relay> {
    let flag = |target: &mut bool, value: Option<Flag>| {
        if let Some(Flag(value)) = value {
            *target = value;
        }
    };

    if let Some(subdomain) = settings.subdomain {
        relay.subdomain = subdomain;
    }
    if let Some(domain) = settings.domain {
        let domain = domain.trim().to_lowercase();
        if domain != relay.domain {
            relay.domain = domain;
            relay.custom_domain_verified = false;
        }
    }
    if let Some(plan_id) = settings.plan_id {
        relay.plan_id = plan_id;
    }
    if let Some(name) = settings.info_name {
        relay.info_name = name;
    }
    if let Some(icon) = settings.info_icon {
        relay.info_icon = icon;
    }
    if let Some(description) = settings.info_description {
        relay.info_description = description;
    }
    flag(&mut relay.policy_public_read, settings.policy_public_read);
    flag(&mut relay.policy_public_write, settings.policy_public_write);
    flag(&mut relay.policy_public_join, settings.policy_public_join);
    flag(
        &mut relay.policy_strip_signatures,
        settings.policy_strip_signatures,
    );
    flag(&mut relay.groups_enabled, settings.groups_enabled);
    flag(&mut relay.management_enabled, settings.management_enabled);
    flag(&mut relay.members_can_invite, settings.members_can_invite);
    flag(&mut relay.blossom_enabled, settings.blossom_enabled);
    flag(&mut relay.livekit_enabled, settings.livekit_enabled);
    flag(&mut relay.push_enabled, settings.push_enabled);
    if let Some(custom_domain) = settings.custom_domain {
        let custom_domain = normalize_custom_domain(&core.config, &custom_domain)?;
        if custom_domain != relay.custom_domain {
            relay.custom_domain = custom_domain;
            relay.custom_domain_verified = false;
        }
    }

    Ok(relay)
}

/// Check a relay's subdomain, domain and plan, and that the plan allows the
/// premium features it has on.
fn validate_relay(core: &Core, relay: Relay) -> Result<Relay> {
    if !SUBDOMAIN_RE.is_match(&relay.subdomain)
        || RESERVED_SUBDOMAINS.contains(&relay.subdomain.as_str())
    {
        return Err(Error::invalid("invalid-subdomain", "subdomain is invalid"));
    }

    if !core.config.relay_domains.contains(&relay.domain) {
        return Err(Error::invalid(
            "invalid-zooid-domain",
            "zooid_domain is not a domain this platform serves",
        ));
    }

    let plan = plans::get_plan(&relay.plan_id)
        .ok_or_else(|| Error::invalid("invalid-plan", "plan not found"))?;
    if (!plan.blossom && relay.blossom_enabled) || (!plan.livekit && relay.livekit_enabled) {
        return Err(Error::invalid(
            "premium-feature",
            "feature requires a paid plan",
        ));
    }

    Ok(relay)
}

/// A taken subdomain or custom domain is the caller's to fix.
fn map_relay_write_error(error: Error) -> Error {
    let constraint = match &error {
        Error::Internal(inner) => inner
            .downcast_ref::<sqlx::Error>()
            .and_then(caravel_db::unique_violation)
            .map(str::to_string),
        _ => None,
    };

    match constraint.as_deref() {
        Some("relay_subdomain_key") => {
            Error::invalid("subdomain-exists", "subdomain already exists")
        }
        Some("relay_custom_domain_key") => {
            Error::invalid("domain-exists", "custom domain is taken")
        }
        _ => error,
    }
}
