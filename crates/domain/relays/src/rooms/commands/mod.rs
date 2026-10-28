//! Applying room events to their rooms, and rewriting the events the relay
//! publishes about each room.

mod apply;
mod publish;

pub use publish::{publish_admins, publish_members, publish_metadata, publish_pins};

use coracle_lib::events::Event;
use sqlx::PgConnection;

use caravel_util::error::Result;
use caravel_util::nostr::Key;

use super::queries;
use crate::rooms::room_id;
use caravel_util::nostr::kinds::rooms as kinds;

/// Apply a room event a client just published to its room, returning the events
/// the relay signed to publish the change. The caller has already checked that the
/// author may publish it.
pub async fn apply_event(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    event: &Event,
    now: i64,
) -> Result<Vec<Event>> {
    let Some(room_id) = room_id(event) else {
        return Ok(Vec::new());
    };

    match event.kind {
        kinds::CREATE_GROUP | kinds::EDIT_METADATA => {
            apply::metadata(&mut *conn, relay_id, room_id, event).await?;
            Ok(vec![
                publish::publish_metadata(&mut *conn, key, relay_id, room_id, now).await?,
                publish::publish_admins(conn, key, relay_id, room_id, now).await?,
            ])
        }
        kinds::PUT_USER => {
            apply::users(&mut *conn, relay_id, room_id, event, now).await?;
            publish::publish_membership(conn, key, relay_id, room_id, now).await
        }
        kinds::ASSIGN_PERMISSION => {
            apply::grant_permissions(&mut *conn, relay_id, room_id, event, now).await?;
            publish::publish_membership(conn, key, relay_id, room_id, now).await
        }
        kinds::UNASSIGN_PERMISSION => {
            apply::revoke_permissions(&mut *conn, relay_id, room_id, event).await?;
            Ok(vec![
                publish::publish_admins(conn, key, relay_id, room_id, now).await?,
            ])
        }
        kinds::REMOVE_USER => {
            apply::remove_users(&mut *conn, relay_id, room_id, event).await?;
            publish::publish_membership(conn, key, relay_id, room_id, now).await
        }
        kinds::JOIN_REQUEST => {
            if !may_join(&mut *conn, relay_id, room_id, event).await? {
                return Ok(Vec::new());
            }
            let pubkey = event.pubkey.to_hex();
            if !apply::join(&mut *conn, relay_id, room_id, &pubkey, now).await? {
                return Ok(Vec::new());
            }
            publish::publish_member_change(
                conn,
                key,
                relay_id,
                room_id,
                &pubkey,
                kinds::PUT_USER,
                now,
            )
            .await
        }
        kinds::LEAVE_REQUEST => {
            let pubkey = event.pubkey.to_hex();
            if !apply::leave(&mut *conn, relay_id, room_id, &pubkey).await? {
                return Ok(Vec::new());
            }
            publish::publish_member_change(
                conn,
                key,
                relay_id,
                room_id,
                &pubkey,
                kinds::REMOVE_USER,
                now,
            )
            .await
        }
        kinds::CREATE_INVITE => {
            apply::invites(&mut *conn, relay_id, room_id, event, now).await?;
            Ok(Vec::new())
        }
        kinds::PUT_PINS => {
            apply::pins(&mut *conn, relay_id, room_id, event).await?;
            Ok(vec![
                publish::publish_pins(conn, key, relay_id, room_id, now).await?,
            ])
        }
        kinds::DELETE_EVENT => {
            apply::delete_events(conn, relay_id, room_id, event).await?;
            Ok(Vec::new())
        }
        kinds::DELETE_GROUP => {
            apply::delete_room(conn, relay_id, room_id).await?;
            Ok(Vec::new())
        }
        _ => Ok(Vec::new()),
    }
}

/// Whether the room lets this join in: it exists, and unless it is closed, or the
/// author holds an invite to it.
async fn may_join(
    conn: &mut PgConnection,
    relay_id: &str,
    room_id: &str,
    event: &Event,
) -> Result<bool> {
    let room = queries::get_room(&mut *conn, relay_id, room_id).await?;
    if room.is_none() {
        return Ok(false);
    }
    if room.is_some_and(|room| !room.has_flag("closed")) {
        return Ok(true);
    }
    queries::is_invited(conn, relay_id, room_id, event).await
}
