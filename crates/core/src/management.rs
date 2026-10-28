//! Managing a relay's access and rooms: its members, NIP-86 calls, and room
//! moderation from the dashboards.

use caravel_util::db::Connection;
use coracle_lib::events::Event;
use serde_json::{Value, json};

use crate::Core;
use caravel_db::{commands, queries};
use caravel_domain as domain;
use caravel_util::error::{Error, Result};
use caravel_domain::management::MANAGED_METHODS;
use caravel_domain::management::{Call, Outcome};
use caravel_domain::memberships::MemberSource;
use caravel_domain::relays::Relay;
use caravel_util::bus::Change;
use caravel_util::encoding::is_hex64;
use caravel_util::nostr::Key;

/// Carry out a NIP-86 call on a relay, whose caller may make it, and return its
/// answer. A change to the relay's access lists or rooms goes out on the bus.
pub async fn call(core: &Core, relay: &Relay, call: Call) -> Result<Value> {
    let outcome = run_call(core, relay, call).await?;
    let result = outcome.result();
    if let Outcome::AccessChanged(published) = outcome {
        core.bus.publish(Change::AccessChanged {
            relay_id: relay.id.clone(),
            published,
        });
    }
    Ok(result)
}

/// Carry out a NIP-86 call on a relay, whose caller may make it, for the relay
/// runtime, which applies an AccessChanged outcome itself before answering.
pub async fn run_call(core: &Core, relay: &Relay, call: Call) -> Result<Outcome> {
    let pool = core.db.pool();
    let relay_id = relay.id.as_str();
    let tenant_pubkey = relay.tenant_pubkey.as_str();
    let now = core.clock.now();

    match &call {
        Call::ListBannedPubkeys => {
            let bans = queries::bans::list_banned_pubkeys(pool, relay_id).await?;
            Ok(Outcome::Answer(json!(
                bans.into_iter()
                    .map(|ban| json!({"pubkey": ban.subject, "reason": ban.reason}))
                    .collect::<Vec<_>>()
            )))
        }
        Call::ListAllowedPubkeys => {
            let members = queries::memberships::list_members(pool, relay_id).await?;
            Ok(Outcome::Answer(json!(
                members
                    .into_iter()
                    .map(|member| json!({"pubkey": member.pubkey, "reason": "relay member"}))
                    .collect::<Vec<_>>()
            )))
        }
        Call::ListBannedEvents => {
            let bans = queries::bans::list_banned_events(pool, relay_id).await?;
            Ok(Outcome::Answer(json!(
                bans.into_iter()
                    .map(|ban| json!({"id": ban.subject, "reason": ban.reason}))
                    .collect::<Vec<_>>()
            )))
        }
        Call::ListMethodAssignees => {
            let all: Vec<&str> = MANAGED_METHODS.to_vec();
            let mut assignees = vec![
                json!({"pubkey": tenant_pubkey, "methods": all}),
                json!({"pubkey": relay.pubkey, "methods": all}),
            ];
            for (pubkey, methods) in queries::management::list_method_grants(pool, relay_id).await?
            {
                if pubkey != tenant_pubkey && pubkey != relay.pubkey {
                    assignees.push(json!({"pubkey": pubkey, "methods": methods}));
                }
            }
            assignees.sort_by(|a, b| a["pubkey"].as_str().cmp(&b["pubkey"].as_str()));
            Ok(Outcome::Answer(json!(assignees)))
        }
        Call::ListClaims => Ok(Outcome::Answer(json!(
            queries::claims::list_claims(pool, relay_id).await?
        ))),
        Call::ChangeRelayName(name) => change_info(core, relay_id, Some(name), None, None).await,
        Call::ChangeRelayDescription(description) => {
            change_info(core, relay_id, None, Some(description), None).await
        }
        Call::ChangeRelayIcon(icon) => change_info(core, relay_id, None, None, Some(icon)).await,
        Call::UnbanPubkey(pubkey) => {
            commands::bans::unban_pubkey(pool, relay_id, pubkey).await?;
            Ok(Outcome::AccessChanged(Vec::new()))
        }
        Call::AllowEvent(id) => {
            commands::bans::allow_event(pool, relay_id, id).await?;
            Ok(Outcome::AccessChanged(Vec::new()))
        }
        Call::CreateClaim(code) => {
            commands::claims::create_claim(pool, relay_id, code, now).await?;
            Ok(Outcome::AccessChanged(Vec::new()))
        }
        Call::DeleteClaim(code) => {
            commands::claims::revoke_claim(pool, relay_id, code, now).await?;
            Ok(Outcome::AccessChanged(Vec::new()))
        }
        Call::BanPubkey(..)
        | Call::AllowPubkey(_)
        | Call::UnallowPubkey(_)
        | Call::BanEvent(..)
        | Call::CreateRole(_)
        | Call::EditRole(_)
        | Call::DeleteRole(_)
        | Call::AssignRole(..)
        | Call::UnassignRole(..)
        | Call::AssignMethod(..)
        | Call::UnassignMethod(..) => {
            let key = core.relay_key(relay)?;
            let published = core
                .db
                .with_tx(async |conn| write(conn, &key, relay, &call, now).await)
                .await?;
            Ok(Outcome::AccessChanged(published))
        }
    }
}

/// Publish a NIP-29 moderation event as the relay itself, for whoever manages it
/// from the dashboards, and apply it to its room. The relay may publish any
/// moderation kind, so only the room rules on what the event says can refuse it.
pub async fn moderate_room(
    core: &Core,
    relay: &Relay,
    kind: u16,
    tags: Vec<Vec<String>>,
) -> Result<()> {
    relay.require_unpurged()?;
    if !domain::rooms::is_moderation_kind(kind) {
        return Err(Error::invalid(
            "invalid-kind",
            format!("{kind} is not a group management kind"),
        ));
    }

    let key = core.relay_key(relay)?;
    let now = core.clock.now();
    let event = key.sign(kind, tags, "", now);
    let snapshot = queries::policy::load_snapshot(core.db.pool(), relay).await?;
    snapshot
        .check_room_write(&event)
        .map_err(|refusal| Error::invalid("refused", refusal.message()))?;

    let mut published = core
        .db
        .with_tx(async |conn| {
            commands::events::save_event(&mut *conn, &relay.id, &event, now).await?;
            commands::rooms::apply_event(conn, &key, &relay.id, &event, now).await
        })
        .await?;
    published.insert(0, event);

    core.bus.publish(Change::AccessChanged {
        relay_id: relay.id.clone(),
        published,
    });
    Ok(())
}

/// Change what the relay says about itself, which reloads it from its row.
async fn change_info(
    core: &Core,
    relay_id: &str,
    name: Option<&str>,
    description: Option<&str>,
    icon: Option<&str>,
) -> Result<Outcome> {
    commands::relays::set_relay_info(core.db.pool(), relay_id, name, description, icon).await?;
    core.bus.publish(Change::RelayChanged {
        relay_id: relay_id.to_string(),
    });
    Ok(Outcome::Answer(json!(true)))
}

/// The calls that change what the relay publishes about itself.
async fn write(
    conn: &mut Connection,
    key: &Key,
    relay: &Relay,
    call: &Call,
    now: i64,
) -> Result<Vec<Event>> {
    let relay_id = relay.id.as_str();
    let tenant_pubkey = relay.tenant_pubkey.as_str();

    match call {
        Call::BanPubkey(pubkey, reason) => {
            commands::bans::ban_pubkey(conn, key, relay_id, tenant_pubkey, pubkey, reason, now)
                .await
        }
        Call::AllowPubkey(pubkey) => {
            commands::memberships::allow_pubkey(conn, key, relay_id, pubkey, now).await
        }
        Call::UnallowPubkey(pubkey) => {
            commands::memberships::remove(conn, key, relay_id, tenant_pubkey, pubkey, now).await
        }
        Call::BanEvent(id, reason) => {
            commands::bans::ban_event(conn, relay_id, id, reason, now).await?;
            Ok(Vec::new())
        }
        Call::CreateRole(role) => {
            commands::roles::create_role(conn, key, relay_id, role, now).await
        }
        Call::EditRole(role) => commands::roles::edit_role(conn, key, relay_id, role, now).await,
        Call::DeleteRole(id) => commands::roles::delete_role(conn, key, relay_id, id, now).await,
        Call::AssignRole(pubkey, role_id) => {
            commands::roles::assign_role(conn, key, relay_id, pubkey, role_id, now).await
        }
        Call::UnassignRole(pubkey, role_id) => {
            commands::roles::unassign_role(conn, key, relay_id, pubkey, role_id, now).await
        }
        Call::AssignMethod(pubkey, method) => {
            commands::management::grant_method(
                conn,
                key,
                relay_id,
                tenant_pubkey,
                pubkey,
                method,
                now,
            )
            .await
        }
        Call::UnassignMethod(pubkey, method) => {
            commands::management::revoke_method(
                conn,
                relay_id,
                tenant_pubkey,
                &relay.pubkey,
                pubkey,
                method,
            )
            .await?;
            Ok(Vec::new())
        }
        _ => unreachable!("{call:?} doesn't write in a transaction"),
    }
}

/// Make a pubkey a member of a relay by hand.
pub async fn add_member(core: &Core, relay: &Relay, pubkey: &str) -> Result<()> {
    relay.require_unpurged()?;
    if !is_hex64(pubkey) {
        return Err(Error::invalid(
            "invalid-pubkey",
            "pubkey must be 64 hex characters",
        ));
    }

    let pubkey = pubkey.to_lowercase();
    let key = core.relay_key(relay)?;
    let now = core.clock.now();
    let published = core
        .db
        .with_tx(async |conn| {
            commands::memberships::admit(conn, &key, &relay.id, &pubkey, MemberSource::Manual, now)
                .await
        })
        .await?;

    core.bus.publish(Change::AccessChanged {
        relay_id: relay.id.clone(),
        published,
    });
    Ok(())
}

/// Remove a member from a relay. Its owner can't be removed.
pub async fn remove_member(core: &Core, relay: &Relay, pubkey: &str) -> Result<()> {
    relay.require_unpurged()?;
    let key = core.relay_key(relay)?;
    let now = core.clock.now();
    let published = core
        .db
        .with_tx(async |conn| {
            commands::memberships::remove(conn, &key, &relay.id, &relay.tenant_pubkey, pubkey, now)
                .await
        })
        .await?;

    core.bus.publish(Change::AccessChanged {
        relay_id: relay.id.clone(),
        published,
    });
    Ok(())
}
