//! Hosting: the operations tenants and admins perform on tenants and relays, each
//! of which spans the relay row, its activity, billing and the relay's hosts.
//! Callers authorize before calling.

use crate::Core;
use crate::billing;
use caravel_db::{commands, queries};
use caravel_domain as domain;
use caravel_domain::activity::ActivityType;
use caravel_domain::memberships::MemberSource;
use caravel_domain::relays::{Relay, RelaySettings, RelayStatus};
use caravel_domain::tenants::Tenant;
use caravel_util::bus::Change;
use caravel_util::error::{Error, Result};
use caravel_util::nostr::generate_keypair;

// --- Tenants ---

/// The tenant for `pubkey`, created with a Stripe customer if it doesn't exist.
/// An existing tenant keeps the `return_url` it was created with.
pub async fn ensure_tenant(core: &Core, pubkey: &str, return_url: &str) -> Result<Tenant> {
    let return_url = return_url.trim().trim_end_matches('/');
    if return_url.is_empty() {
        return Err(Error::invalid(
            "return-url-required",
            "return_url is required",
        ));
    }

    if let Some(tenant) = queries::tenants::get_tenant(core.db.pool(), pubkey).await? {
        return Ok(tenant);
    }

    let name = core
        .robot
        .fetch_nostr_name(pubkey, core.clock.now())
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
    commands::tenants::insert_tenant(core.db.pool(), &tenant).await?;

    queries::tenants::get_tenant(core.db.pool(), pubkey)
        .await?
        .ok_or_else(|| Error::internal("tenant missing after insert"))
}

/// Replace the tenant's NWC url, stored encrypted. Empty removes it. Saving one
/// brings a churned tenant back, as new activity does; the next reconcile prices
/// their restored relays.
pub async fn set_tenant_nwc_url(core: &Core, pubkey: &str, nwc_url: &str) -> Result<()> {
    let encrypted = if nwc_url.is_empty() {
        None
    } else {
        Some(core.config.platform_key.encrypt(nwc_url)?)
    };

    commands::tenants::set_tenant_nwc_url(core.db.pool(), pubkey, encrypted.as_deref()).await?;
    if encrypted.is_some() {
        billing::reactivate_tenant(core, pubkey).await?;
    }
    Ok(())
}

/// Ban a tenant: block relay creation, then forgive what they owe and purge their
/// relays as deleting them does.
pub async fn ban_tenant(core: &Core, pubkey: &str) -> Result<()> {
    let now = core.clock.now();
    let purged = core
        .db
        .with_tx(async |conn| {
            let tenant = queries::tenants::lock_tenant(conn, pubkey).await?;
            if tenant.banned_at.is_some() {
                return Err(Error::conflict(
                    "tenant-is-banned",
                    "tenant is already banned",
                ));
            }

            commands::tenants::set_tenant_banned_at(&mut *conn, pubkey, Some(now)).await?;
            commands::tenants::forgive_and_purge_in_tx(conn, pubkey, now).await
        })
        .await?;

    for relay_id in purged {
        core.bus.publish(Change::RelayChanged { relay_id });
    }
    Ok(())
}

/// Delete a tenant without banning them: forgive what they owe and purge their
/// relays. They may sign in and create relays again, under the same subdomains.
pub async fn delete_tenant(core: &Core, pubkey: &str) -> Result<()> {
    let now = core.clock.now();
    let purged = core
        .db
        .with_tx(async |conn| {
            queries::tenants::lock_tenant(conn, pubkey).await?;
            commands::tenants::forgive_and_purge_in_tx(conn, pubkey, now).await
        })
        .await?;

    for relay_id in purged {
        core.bus.publish(Change::RelayChanged { relay_id });
    }
    Ok(())
}

/// Lift a tenant's ban. Purged relays stay purged and forgiven charges forgiven.
pub async fn unban_tenant(core: &Core, pubkey: &str) -> Result<()> {
    let tenant = queries::tenants::require_tenant(core.db.pool(), pubkey).await?;
    if tenant.banned_at.is_none() {
        return Err(Error::conflict("tenant-not-banned", "tenant is not banned"));
    }

    commands::tenants::set_tenant_banned_at(core.db.pool(), pubkey, None).await
}

// --- Relays ---

/// Create a relay for a tenant, with a keypair of its own. A custom domain another
/// relay has verified is refused.
pub async fn create_relay(
    core: &Core,
    tenant_pubkey: &str,
    settings: RelaySettings,
) -> Result<Relay> {
    let tenant = queries::tenants::require_tenant(core.db.pool(), tenant_pubkey).await?;
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
    let relay = relay
        .apply_settings(&core.config, settings)?
        .validate(&core.config)?;
    if let Some(custom_domain) = &relay.custom_domain
        && queries::relays::is_custom_domain_taken(core.db.pool(), custom_domain, &relay.id).await?
    {
        return Err(Error::invalid("domain-exists", "custom domain is taken"));
    }
    let key = core.relay_key(&relay)?;

    core.db
        .with_tx(async |conn| {
            commands::relays::insert_relay(conn, &relay).await?;
            commands::activity::insert_relay_activity(
                &mut *conn,
                ActivityType::CreateRelay,
                &relay,
                relay.status,
                now,
            )
            .await?;
            commands::memberships::admit(
                conn,
                &key,
                &relay.id,
                tenant_pubkey,
                MemberSource::Owner,
                now,
            )
            .await
        })
        .await?;

    core.bus.publish(Change::RelayChanged {
        relay_id: relay.id.clone(),
    });

    queries::relays::get_relay(core.db.pool(), &relay.id)
        .await?
        .ok_or_else(|| Error::internal("relay missing after insert"))
}

/// Change a relay's settings. Moving it to another host or replacing its custom
/// domain withdraws the custom domain's verification; a replaced verified domain
/// redirects to the relay from then on. A new custom domain another relay has
/// verified is refused, as is downgrading to a plan with a member limit the relay
/// is over.
pub async fn update_relay(core: &Core, relay: Relay, settings: RelaySettings) -> Result<Relay> {
    relay.require_unpurged()?;

    let previous = relay.clone();
    let relay = relay
        .apply_settings(&core.config, settings)?
        .validate(&core.config)?;

    if relay.custom_domain != previous.custom_domain
        && let Some(custom_domain) = &relay.custom_domain
        && queries::relays::is_custom_domain_taken(core.db.pool(), custom_domain, &relay.id).await?
    {
        return Err(Error::invalid("domain-exists", "custom domain is taken"));
    }

    if relay.plan_id != previous.plan_id
        && let Some(limit) = relay.member_limit()
    {
        let members = queries::memberships::count_members(core.db.pool(), &relay.id).await?;
        if members > limit {
            return Err(Error::invalid(
                "member-limit-exceeded",
                format!(
                    "relay has {members} members, which exceeds the {} plan limit of {limit}",
                    relay.plan_id
                ),
            ));
        }
    }

    let now = core.clock.now();
    let replaced_domain = previous.custom_domain.clone().filter(|_| {
        previous.custom_domain_verified && previous.custom_domain != relay.custom_domain
    });
    let mut own_hosts = vec![relay.canonical_host()];
    own_hosts.extend(relay.custom_domain.clone());

    core.db
        .with_tx(async |conn| {
            commands::relays::update_relay(conn, &relay).await?;
            if let Some(host) = &replaced_domain {
                commands::relays::add_host_redirect(&mut *conn, host, &relay.id, now).await?;
            }
            commands::relays::remove_host_redirects(&mut *conn, &relay.id, &own_hosts).await?;
            commands::activity::insert_relay_activity(
                conn,
                ActivityType::UpdateRelay,
                &relay,
                relay.status,
                now,
            )
            .await
        })
        .await?;

    core.bus.publish(Change::RelayChanged {
        relay_id: relay.id.clone(),
    });

    queries::relays::require_relay(core.db.pool(), &relay.id).await
}

/// Pause a relay at its tenant's request, crediting the rest of the period.
pub async fn deactivate_relay(core: &Core, relay: &Relay) -> Result<()> {
    change_relay_status(
        core,
        relay,
        RelayStatus::Inactive,
        ActivityType::DeactivateRelay,
    )
    .await
}

/// Resume a paused relay, charging the rest of the period.
pub async fn reactivate_relay(core: &Core, relay: &Relay) -> Result<()> {
    change_relay_status(
        core,
        relay,
        RelayStatus::Active,
        ActivityType::ActivateRelay,
    )
    .await
}

/// Purge a relay: delete its data and release its hosts for good, leaving its
/// subdomain free for another relay. The row stays.
pub async fn purge_relay(core: &Core, relay: &Relay) -> Result<()> {
    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            // Judged on the locked row, so a stale copy can't credit an active relay twice.
            let relay = queries::relays::lock_relay(conn, &relay.id).await?;
            if relay.is_purged() {
                return Err(Error::conflict(
                    "relay-is-purged",
                    "relay is already purged",
                ));
            }

            commands::relays::purge_relay_in_tx(conn, &relay, now).await
        })
        .await?;

    core.bus.publish(Change::RelayChanged {
        relay_id: relay.id.clone(),
    });
    Ok(())
}

/// Erase what a relay holds, keeping its settings: its events, media, rooms, roles,
/// method grants, bans, claims, notifications and members. Its owner stays, as do
/// the members and invites someone has paid for, and the relay republishes its
/// member list.
pub async fn erase_relay(core: &Core, relay: &Relay) -> Result<()> {
    relay.require_unpurged()?;

    let key = core.relay_key(relay)?;
    let now = core.clock.now();
    let (published, blobs) = core
        .db
        .with_tx(async |conn| {
            // Joins bind paid invites under this lock.
            queries::relays::lock_relay(conn, &relay.id).await?;

            let blobs = commands::relays::erase_relay_data(&mut *conn, &relay.id).await?;
            let published = commands::relays::rebuild_derived(conn, &key, &relay.id, now).await?;
            Ok::<_, Error>((published, blobs))
        })
        .await?;

    core.bus.publish(Change::AccessChanged {
        relay_id: relay.id.clone(),
        published,
    });

    // Bytes left behind go when the relay is purged.
    for sha256 in blobs {
        let key = domain::media::blob_key(&relay.id, &sha256);
        // A same-blob upload since the commit keeps its bytes; uploads hold this lock.
        let _lock = core.blobs.lock(&key).await;
        let gone = matches!(
            queries::media::get_blob(core.db.pool(), &relay.id, &sha256).await,
            Ok(None)
        );
        if gone && let Err(error) = core.blobs.delete(&key).await {
            tracing::warn!(relay = %relay.id, error = %error, "failed to delete a blob's bytes");
        }
    }
    Ok(())
}

/// Move a relay between active and inactive, refusing a purged or delinquent relay
/// or one already there. Judged on its locked row, so concurrent requests record
/// the change, and bill it, once.
async fn change_relay_status(
    core: &Core,
    relay: &Relay,
    status: RelayStatus,
    activity_type: ActivityType,
) -> Result<()> {
    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            let relay = queries::relays::lock_relay(conn, &relay.id).await?;
            relay.require_unpurged()?;
            match (relay.status, status) {
                (RelayStatus::Delinquent, _) => Err(Error::conflict(
                    "relay-is-delinquent",
                    "relay is delinquent",
                )),
                (RelayStatus::Active, RelayStatus::Active) => Err(Error::conflict(
                    "relay-is-active",
                    "relay is already active",
                )),
                (RelayStatus::Inactive, RelayStatus::Inactive) => Err(Error::conflict(
                    "relay-is-inactive",
                    "relay is already inactive",
                )),
                _ => {
                    commands::activity::record_status_change(
                        conn,
                        &relay,
                        status,
                        activity_type,
                        now,
                    )
                    .await
                }
            }
        })
        .await?;

    core.bus.publish(Change::RelayChanged {
        relay_id: relay.id.clone(),
    });
    Ok(())
}

/// Erase what purged relays held: their events, access lists, rooms, claims,
/// notifications and media. Billing's records of them stay.
pub async fn erase_purged_relays(core: &Core) -> Result<()> {
    for relay in queries::relays::list_unerased_relays(core.db.pool()).await? {
        core.blobs.delete_prefix(&relay.id).await?;

        let now = core.clock.now();
        core.db
            .with_tx(async |conn| {
                commands::relays::erase_purged_relay_data(&mut *conn, &relay.id).await?;
                commands::relays::set_relay_erased_at(conn, &relay.id, now).await
            })
            .await?;

        tracing::info!(relay = %relay.id, "erased a purged relay");
    }
    Ok(())
}
