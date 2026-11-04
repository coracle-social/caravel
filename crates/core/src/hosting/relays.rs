//! Relays: creating, configuring, pausing and restoring the rows tenants own.

use caravel_billing::relays::{Activity, ActivityType, Relay, RelaySettings, RelayStatus};
use caravel_relays::access::MemberSource;
use caravel_util::bus::Change;
use caravel_util::db::Connection;
use caravel_util::error::{Error, Result};
use caravel_util::nostr::generate_keypair;

use crate::Core;

/// Every relay on the platform.
pub async fn list_relays(core: &Core) -> Result<Vec<Relay>> {
    caravel_billing::relays::queries::list_relays(core.db.pool()).await
}

/// A tenant's relays.
pub async fn list_relays_for_tenant(core: &Core, tenant_pubkey: &str) -> Result<Vec<Relay>> {
    caravel_billing::relays::queries::list_relays_for_tenant(core.db.pool(), tenant_pubkey).await
}

/// The relay with `id`, if there is one.
pub async fn get_relay(core: &Core, id: &str) -> Result<Option<Relay>> {
    caravel_billing::relays::queries::get_relay(core.db.pool(), id).await
}

/// The relay with `id`, or a 404.
pub async fn require_relay(core: &Core, id: &str) -> Result<Relay> {
    caravel_billing::relays::queries::require_relay(core.db.pool(), id).await
}

/// What a relay did that billing prices, newest first.
pub async fn relay_activity(core: &Core, relay_id: &str) -> Result<Vec<Activity>> {
    caravel_billing::relays::queries::activity::list_activity_for_relay(core.db.pool(), relay_id)
        .await
}

/// The relay with `id` if it sells memberships, or a 404.
pub async fn selling_relay(core: &Core, id: &str) -> Result<Relay> {
    caravel_billing::relays::queries::get_relay(core.db.pool(), id)
        .await?
        .filter(Relay::charges_members)
        .ok_or(Error::NotFound("relay not found"))
}

/// Create a relay for a tenant, with a keypair of its own. A custom domain another
/// relay has verified is refused.
pub async fn create_relay(
    core: &Core,
    tenant_pubkey: &str,
    settings: RelaySettings,
) -> Result<Relay> {
    let tenant =
        caravel_billing::tenants::queries::require_tenant(core.db.pool(), tenant_pubkey).await?;
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
    let relay = with_settings(&core.config, relay, settings, &tenant, None)?;
    let key = core.relay_key(&relay)?;

    core.db
        .with_tx(async |conn| {
            require_custom_domain_free(&mut *conn, &relay).await?;
            caravel_billing::relays::commands::insert_relay(conn, &relay).await?;
            caravel_billing::relays::commands::activity::insert_relay_activity(
                &mut *conn,
                ActivityType::CreateRelay,
                &relay,
                relay.status,
                now,
            )
            .await?;
            caravel_relays::access::commands::admit(
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

    caravel_billing::relays::queries::get_relay(core.db.pool(), &relay.id)
        .await?
        .ok_or_else(|| Error::internal("relay missing after insert"))
}

/// Change a relay's settings. Moving it to another host or replacing its custom
/// domain withdraws the custom domain's verification; a replaced verified domain
/// redirects to the relay from then on. A new custom domain another relay has
/// verified is refused, as is downgrading to a plan with a member limit the relay
/// is over. Judged on the locked row, so a concurrent purge or update is met.
pub async fn update_relay(core: &Core, relay: Relay, settings: RelaySettings) -> Result<Relay> {
    let now = core.clock.now();
    let config = core.config.as_ref();
    let settings = &settings;

    let id = core
        .db
        .with_tx(async |conn| {
            let previous =
                caravel_billing::relays::queries::lock_relay(&mut *conn, &relay.id).await?;
            previous.require_unpurged()?;

            let tenant = caravel_billing::tenants::queries::require_tenant(
                &mut *conn,
                &previous.tenant_pubkey,
            )
            .await?;
            let updated = with_settings(
                config,
                previous.clone(),
                settings.clone(),
                &tenant,
                Some(&previous),
            )?;

            if updated.custom_domain != previous.custom_domain {
                require_custom_domain_free(&mut *conn, &updated).await?;
            }
            if updated.plan_id != previous.plan_id {
                require_members_fit_plan(&mut *conn, &updated).await?;
            }

            let replaced_domain = previous.custom_domain.clone().filter(|_| {
                previous.custom_domain_verified && previous.custom_domain != updated.custom_domain
            });
            let mut own_hosts = vec![updated.canonical_host()];
            own_hosts.extend(updated.custom_domain.clone());

            caravel_billing::relays::commands::update_relay(&mut *conn, &updated).await?;
            if let Some(host) = &replaced_domain {
                caravel_billing::relays::commands::add_host_redirect(
                    &mut *conn,
                    host,
                    &updated.id,
                    now,
                )
                .await?;
            }
            caravel_billing::relays::commands::remove_host_redirects(
                &mut *conn,
                &updated.id,
                &own_hosts,
            )
            .await?;
            caravel_billing::relays::commands::activity::insert_relay_activity(
                conn,
                ActivityType::UpdateRelay,
                &updated,
                updated.status,
                now,
            )
            .await?;

            Ok::<_, Error>(updated.id)
        })
        .await?;

    core.bus.publish(Change::RelayChanged {
        relay_id: id.clone(),
    });

    caravel_billing::relays::queries::require_relay(core.db.pool(), &id).await
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
            let relay = caravel_billing::relays::queries::lock_relay(conn, &relay.id).await?;
            if relay.is_purged() {
                return Err(Error::conflict(
                    "relay-is-purged",
                    "relay is already purged",
                ));
            }

            caravel_billing::relays::commands::purge_relay_in_tx(conn, &relay, now).await
        })
        .await?;

    core.bus.publish(Change::RelayChanged {
        relay_id: relay.id.clone(),
    });
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
            let relay = caravel_billing::relays::queries::lock_relay(conn, &relay.id).await?;
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
                    caravel_billing::relays::commands::activity::record_status_change(
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

/// The relay as these settings would leave it, with the platform's configuration
/// checked.
fn with_settings(
    config: &caravel_util::config::Config,
    relay: Relay,
    settings: RelaySettings,
    tenant: &caravel_billing::tenants::Tenant,
    previous: Option<&Relay>,
) -> Result<Relay> {
    relay
        .apply_settings(config, settings)?
        .validate(config, tenant, previous)
}

/// Refuse a custom domain another relay has verified.
async fn require_custom_domain_free(conn: &mut Connection, relay: &Relay) -> Result<()> {
    let Some(custom_domain) = &relay.custom_domain else {
        return Ok(());
    };
    if caravel_billing::relays::queries::is_custom_domain_taken(
        &mut *conn,
        custom_domain,
        &relay.id,
    )
    .await?
    {
        return Err(Error::invalid("domain-exists", "custom domain is taken"));
    }
    Ok(())
}

/// Refuse a plan whose member limit the relay is over already.
async fn require_members_fit_plan(conn: &mut Connection, relay: &Relay) -> Result<()> {
    let Some(limit) = relay.member_limit() else {
        return Ok(());
    };
    let members = caravel_relays::access::queries::count_members(&mut *conn, &relay.id).await?;
    if members > limit {
        return Err(Error::invalid(
            "member-limit-exceeded",
            format!(
                "relay has {members} members, which exceeds the {} plan limit of {limit}",
                relay.plan_id
            ),
        ));
    }
    Ok(())
}
