//! Managing a relay's access and rooms: its members, NIP-86 calls, and room
//! moderation from the dashboards.

use caravel_util::db::Connection;
use coracle_lib::events::Event;
use serde_json::{Value, json};

use crate::Core;
use caravel_billing::relays::Relay;
use caravel_relays::access::{MemberSource, RelayMember, Role};
use caravel_relays::events::{EventPage, StoredEvent};
use caravel_relays::management::MANAGED_METHODS;
use caravel_relays::management::{Call, Outcome};
use caravel_relays::rooms::{Room, RoomMember};
use caravel_util::bus::Change;
use caravel_util::encoding::is_hex64;
use caravel_util::error::{Error, Result};
use caravel_util::nostr::Key;

// --- Reads ---

/// A relay's members and the roles it defines.
pub async fn access_lists(core: &Core, relay_id: &str) -> Result<(Vec<RelayMember>, Vec<Role>)> {
    let members = caravel_relays::access::queries::list_members(core.db.pool(), relay_id).await?;
    let roles = caravel_relays::access::queries::list_roles(core.db.pool(), relay_id).await?;
    Ok((members, roles))
}

/// A relay's rooms, its rooms' members, and the invite codes of each room.
pub async fn rooms(
    core: &Core,
    relay_id: &str,
) -> Result<(Vec<Room>, Vec<RoomMember>, Vec<(String, String)>)> {
    let rooms = caravel_relays::rooms::queries::list_rooms(core.db.pool(), relay_id).await?;
    let members =
        caravel_relays::rooms::queries::list_room_members(core.db.pool(), relay_id).await?;
    let invites =
        caravel_relays::rooms::queries::list_room_invites(core.db.pool(), relay_id).await?;
    Ok((rooms, members, invites))
}

/// A page of a relay's unexpired events, newest first.
pub async fn page_events(core: &Core, relay: &Relay, page: &EventPage) -> Result<Vec<StoredEvent>> {
    caravel_relays::events::queries::page_events(
        core.db.pool(),
        &relay.id,
        &relay.pubkey,
        page,
        core.clock.now(),
    )
    .await
}

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
            let bans = caravel_relays::access::queries::list_banned_pubkeys(pool, relay_id).await?;
            Ok(Outcome::Answer(json!(
                bans.into_iter()
                    .map(|ban| json!({"pubkey": ban.subject, "reason": ban.reason}))
                    .collect::<Vec<_>>()
            )))
        }
        Call::ListAllowedPubkeys => {
            let members = caravel_relays::access::queries::list_members(pool, relay_id).await?;
            Ok(Outcome::Answer(json!(
                members
                    .into_iter()
                    .map(|member| json!({"pubkey": member.pubkey, "reason": "relay member"}))
                    .collect::<Vec<_>>()
            )))
        }
        Call::ListBannedEvents => {
            let bans = caravel_relays::access::queries::list_banned_events(pool, relay_id).await?;
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
            for (pubkey, methods) in
                caravel_relays::management::queries::list_method_grants(pool, relay_id).await?
            {
                if pubkey != tenant_pubkey && pubkey != relay.pubkey {
                    assignees.push(json!({"pubkey": pubkey, "methods": methods}));
                }
            }
            assignees.sort_by(|a, b| a["pubkey"].as_str().cmp(&b["pubkey"].as_str()));
            Ok(Outcome::Answer(json!(assignees)))
        }
        Call::ListClaims => Ok(Outcome::Answer(json!(
            caravel_relays::access::queries::list_claims(pool, relay_id).await?
        ))),
        Call::ChangeRelayName(name) => change_info(core, relay_id, Some(name), None, None).await,
        Call::ChangeRelayDescription(description) => {
            change_info(core, relay_id, None, Some(description), None).await
        }
        Call::ChangeRelayIcon(icon) => change_info(core, relay_id, None, None, Some(icon)).await,
        Call::UnbanPubkey(pubkey) => {
            caravel_relays::access::commands::unban_pubkey(pool, relay_id, pubkey).await?;
            Ok(Outcome::AccessChanged(Vec::new()))
        }
        Call::AllowEvent(id) => {
            caravel_relays::access::commands::allow_event(pool, relay_id, id).await?;
            Ok(Outcome::AccessChanged(Vec::new()))
        }
        Call::CreateClaim(code) => {
            caravel_relays::access::commands::create_claim(pool, relay_id, code, now).await?;
            Ok(Outcome::AccessChanged(Vec::new()))
        }
        Call::DeleteClaim(code) => {
            caravel_relays::access::commands::revoke_claim(pool, relay_id, code, now).await?;
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
    if !caravel_relays::rooms::is_moderation_kind(kind) {
        return Err(Error::invalid(
            "invalid-kind",
            format!("{kind} is not a group management kind"),
        ));
    }

    let key = core.relay_key(relay)?;
    let now = core.clock.now();
    let event = key.sign(kind, tags, "", now);
    let snapshot =
        caravel_relays::policy::queries::load_snapshot(core.db.pool(), relay.snapshot()).await?;
    snapshot
        .check_room_write(&event)
        .map_err(|refusal| Error::invalid("refused", refusal.message()))?;

    let mut published = core
        .db
        .with_tx(async |conn| {
            caravel_relays::events::commands::save_event(&mut *conn, &relay.id, &event, now)
                .await?;
            caravel_relays::rooms::commands::apply_event(conn, &key, &relay.id, &event, now).await
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
    caravel_billing::relays::commands::set_relay_info(
        core.db.pool(),
        relay_id,
        name,
        description,
        icon,
    )
    .await?;
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
            caravel_relays::access::commands::ban_pubkey(
                conn,
                key,
                relay_id,
                tenant_pubkey,
                pubkey,
                reason,
                now,
            )
            .await
        }
        Call::AllowPubkey(pubkey) => {
            caravel_relays::access::commands::allow_pubkey(conn, key, relay_id, pubkey, now).await
        }
        Call::UnallowPubkey(pubkey) => {
            caravel_relays::access::commands::remove(
                conn,
                key,
                relay_id,
                tenant_pubkey,
                pubkey,
                now,
            )
            .await
        }
        Call::BanEvent(id, reason) => {
            caravel_relays::access::commands::ban_event(conn, relay_id, id, reason, now).await?;
            Ok(Vec::new())
        }
        Call::CreateRole(role) => {
            caravel_relays::access::commands::create_role(conn, key, relay_id, role, now).await
        }
        Call::EditRole(role) => {
            caravel_relays::access::commands::edit_role(conn, key, relay_id, role, now).await
        }
        Call::DeleteRole(id) => {
            caravel_relays::access::commands::delete_role(conn, key, relay_id, id, now).await
        }
        Call::AssignRole(pubkey, role_id) => {
            caravel_relays::access::commands::assign_role(conn, key, relay_id, pubkey, role_id, now)
                .await
        }
        Call::UnassignRole(pubkey, role_id) => {
            caravel_relays::access::commands::unassign_role(
                conn, key, relay_id, pubkey, role_id, now,
            )
            .await
        }
        Call::AssignMethod(pubkey, method) => {
            caravel_relays::management::commands::grant_method(
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
            caravel_relays::management::commands::revoke_method(
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
            caravel_relays::access::commands::admit(
                conn,
                &key,
                &relay.id,
                &pubkey,
                MemberSource::Manual,
                now,
            )
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
            caravel_relays::access::commands::remove(
                conn,
                &key,
                &relay.id,
                &relay.tenant_pubkey,
                pubkey,
                now,
            )
            .await
        })
        .await?;

    core.bus.publish(Change::AccessChanged {
        relay_id: relay.id.clone(),
        published,
    });
    Ok(())
}
