//! Loading what a relay's rules read.

use sqlx::PgExecutor;

use crate::error::Result;
use crate::models::policy::{RelaySnapshot, RoomSnapshot};
use crate::models::relays::Relay;
use crate::queries;

/// Load a relay's snapshot: its settings, members, method grants, bans and rooms.
pub async fn load_snapshot(db: impl PgExecutor<'_> + Copy, relay: &Relay) -> Result<RelaySnapshot> {
    let mut snapshot = RelaySnapshot::of(relay);

    snapshot.members = queries::members::list_members(db, &relay.id)
        .await?
        .into_iter()
        .map(|member| member.pubkey)
        .collect();
    snapshot.method_grants = queries::methods::list_method_grants(db, &relay.id)
        .await?
        .into_iter()
        .map(|(pubkey, methods)| (pubkey, methods.into_iter().collect()))
        .collect();
    snapshot.banned_pubkeys = queries::bans::list_banned_pubkeys(db, &relay.id)
        .await?
        .into_iter()
        .map(|ban| ban.subject)
        .collect();
    snapshot.banned_events = queries::bans::list_banned_events(db, &relay.id)
        .await?
        .into_iter()
        .map(|ban| ban.subject)
        .collect();

    for room in queries::rooms::list_rooms(db, &relay.id).await? {
        let room_snapshot = RoomSnapshot {
            hidden: room.has_flag("hidden"),
            private: room.has_flag("private"),
            closed: room.has_flag("closed"),
            livekit: room.has_flag("livekit"),
            restricted: room.has_flag("restricted"),
            ..Default::default()
        };
        snapshot.rooms.insert(room.id, room_snapshot);
    }
    for member in queries::rooms::list_room_members(db, &relay.id).await? {
        if let Some(room) = snapshot.rooms.get_mut(&member.room_id) {
            let permissions = member
                .permissions
                .iter()
                .filter_map(|kind| u16::try_from(*kind).ok());
            room.permissions
                .insert(member.pubkey.clone(), permissions.collect());
            room.members.insert(member.pubkey);
        }
    }
    for (room_id, code) in queries::rooms::list_room_invites(db, &relay.id).await? {
        if let Some(room) = snapshot.rooms.get_mut(&room_id) {
            room.invites.insert(code);
        }
    }

    Ok(snapshot)
}
