//! Reading what a relay's dashboards show: its access lists, its rooms and
//! its events.

use caravel_billing::relays::Relay;
use caravel_util::error::Result;

use caravel_relays::access::{RelayMember, Role};
use caravel_relays::events::{EventPage, StoredEvent};
use caravel_relays::rooms::{Room, RoomMember};

use crate::Core;

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
