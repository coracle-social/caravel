//! Rooms: the NIP-29 groups inside a relay. Clients' moderation events stay stored
//! as history; each one's effect on its room is projected into tables in the
//! transaction that stores it, and the relay-signed events describing the room are
//! rewritten from those tables.

use serde::{Deserialize, Serialize};
use sqlx::types::Json;

use caravel_util::nostr::kinds::rooms as kinds;

/// Kinds that change a room, each of which only the relay's managers and the room
/// members permitted that kind may publish.
pub const MODERATION_KINDS: [u16; 10] = [
    kinds::PUT_USER,
    kinds::REMOVE_USER,
    kinds::EDIT_METADATA,
    kinds::DELETE_EVENT,
    kinds::CREATE_GROUP,
    kinds::DELETE_GROUP,
    kinds::CREATE_INVITE,
    kinds::PUT_PINS,
    kinds::ASSIGN_PERMISSION,
    kinds::UNASSIGN_PERMISSION,
];

/// A room's settings, as its creator and moderators set them.
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Room {
    pub id: String,
    /// The metadata tags from the room's latest create or edit event, without its
    /// `h` tag: its name, picture and about, and the flags `hidden`, `private`
    /// and `closed`.
    pub tags: Json<Vec<Vec<String>>>,
    /// The `e` and `a` tags of the room's pinned events.
    pub pins: Json<Vec<Vec<String>>>,
    pub created_at: i64,
}

impl Room {
    pub fn has_flag(&self, name: &str) -> bool {
        self.tags
            .iter()
            .any(|tag| tag.first().map(String::as_str) == Some(name))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct RoomMember {
    pub room_id: String,
    pub pubkey: String,
    /// Labels the room's moderators give the member, which grant nothing.
    pub roles: Vec<String>,
    /// The moderation kinds the member may publish in the room.
    pub permissions: Vec<i32>,
    pub created_at: i64,
}
