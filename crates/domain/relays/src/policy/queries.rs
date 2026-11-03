//! Loading what a relay's rules read.

use sqlx::PgExecutor;

use caravel_util::error::Result;
use caravel_util::nostr::keys::parse_pubkey;

use super::{RelaySnapshot, RoomSnapshot};
use crate::{access, management, rooms};

/// Load a relay's snapshot: its settings, members, method grants, bans and rooms.
/// The caller starts the snapshot from the relay's settings; this fills in what
/// the database knows.
pub async fn load_snapshot(
    db: impl PgExecutor<'_> + Copy,
    mut snapshot: RelaySnapshot,
) -> Result<RelaySnapshot> {
    let relay_id = snapshot.relay_id.clone();

    snapshot.members = access::queries::list_members(db, &relay_id)
        .await?
        .into_iter()
        .map(|member| parse_pubkey(&member.pubkey))
        .collect::<Result<_>>()?;
    snapshot.method_grants = management::queries::list_method_grants(db, &relay_id)
        .await?
        .into_iter()
        .map(|(held_by, methods)| Ok((parse_pubkey(&held_by)?, methods.into_iter().collect())))
        .collect::<Result<_>>()?;
    snapshot.banned_pubkeys = access::queries::list_banned_pubkeys(db, &relay_id)
        .await?
        .into_iter()
        .map(|ban| parse_pubkey(&ban.subject))
        .collect::<Result<_>>()?;
    snapshot.banned_events = access::queries::list_banned_events(db, &relay_id)
        .await?
        .into_iter()
        .map(|ban| ban.subject)
        .collect();

    for room in rooms::queries::list_rooms(db, &relay_id).await? {
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
    for member in rooms::queries::list_room_members(db, &relay_id).await? {
        if let Some(room) = snapshot.rooms.get_mut(&member.room_id) {
            let permissions = member
                .permissions
                .iter()
                .filter_map(|kind| u16::try_from(*kind).ok());
            let pubkey = parse_pubkey(&member.pubkey)?;
            room.permissions.insert(pubkey, permissions.collect());
            room.members.insert(pubkey);
        }
    }
    for (room_id, code) in rooms::queries::list_room_invites(db, &relay_id).await? {
        if let Some(room) = snapshot.rooms.get_mut(&room_id) {
            room.invites.insert(code);
        }
    }

    Ok(snapshot)
}
