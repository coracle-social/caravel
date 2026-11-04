//! The NIP-86 calls that change what the relay publishes about itself.

use caravel_billing::relays::Relay;
use caravel_util::error::Result;
use serde_json::json;

use caravel_relays::management::{Call, Outcome};
use caravel_util::bus::Change;
use caravel_util::db::Connection;
use caravel_util::nostr::Key;
use coracle_lib::events::Event;

use crate::Core;

/// The calls that change what the relay publishes about itself.
pub(super) async fn write(
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
            caravel_billing::relays::queries::membership::require_seat(
                &mut *conn,
                relay,
                Some(pubkey),
            )
            .await?;
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
            // Assigning a role admits the pubkey, so it takes a seat like any add.
            caravel_billing::relays::queries::membership::require_seat(
                &mut *conn,
                relay,
                Some(pubkey),
            )
            .await?;
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
            // Granting a method admits the pubkey, so it takes a seat like any add.
            caravel_billing::relays::queries::membership::require_seat(
                &mut *conn,
                relay,
                Some(pubkey),
            )
            .await?;
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

/// Change what the relay says about itself, which reloads it from its row.
pub(super) async fn change_info(
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
