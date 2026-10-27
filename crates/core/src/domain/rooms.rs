//! Which events belong to the room machinery, and which room each is about.

use coracle_lib::events::Event;

use crate::domain;
use crate::models::rooms::{MODERATION_KINDS, RoomMember, kinds};

/// Kinds the relay publishes about a room, addressed by its `d` tag.
pub fn is_metadata_kind(kind: u16) -> bool {
    (kinds::METADATA..=kinds::PINS).contains(&kind)
}

pub fn is_moderation_kind(kind: u16) -> bool {
    MODERATION_KINDS.contains(&kind)
}

/// The kinds a permission tag (`["p", <pubkey>, <kind>...]`) grants or revokes, or
/// `None` if one isn't a moderation kind.
pub fn permission_kinds(tag: &[String]) -> Option<Vec<u16>> {
    tag.get(2..)
        .unwrap_or_default()
        .iter()
        .map(|kind| {
            kind.parse::<u16>()
                .ok()
                .filter(|kind| is_moderation_kind(*kind))
        })
        .collect()
}

/// Whether an event belongs to the room machinery: it is about a room, changes
/// one, asks to join or leave one, or is posted in one.
pub fn is_room_event(kind: u16, room: Option<&str>) -> bool {
    is_metadata_kind(kind)
        || is_moderation_kind(kind)
        || matches!(kind, kinds::JOIN_REQUEST | kinds::LEAVE_REQUEST)
        || room.is_some()
}

/// The tag naming the room an event of `kind` is about: `d` for the relay's room
/// events, and `h` otherwise.
pub fn room_tag(kind: u16) -> &'static str {
    if is_metadata_kind(kind) { "d" } else { "h" }
}

/// The room an event is about.
pub fn room_id(event: &Event) -> Option<&str> {
    event
        .tags
        .value(room_tag(event.kind))
        .filter(|id| !id.is_empty())
}

/// The tags of a room's admin list (kind 39001), naming the moderation kinds each
/// admin may publish: the relay's owner, who may publish all of them in every
/// room, then the room's members permitted any.
pub fn admins_tags(room_id: &str, owner: &str, members: &[RoomMember]) -> Vec<Vec<String>> {
    let admin = |pubkey: &str, kinds: Vec<String>| {
        let mut tag = vec!["p".to_string(), pubkey.to_string()];
        tag.extend(kinds);
        tag
    };

    let mut tags = vec![
        domain::events::tag(&["-"]),
        domain::events::tag(&["d", room_id]),
    ];
    tags.push(admin(
        owner,
        MODERATION_KINDS.iter().map(u16::to_string).collect(),
    ));
    for member in members {
        if member.pubkey != owner && !member.permissions.is_empty() {
            tags.push(admin(
                &member.pubkey,
                member.permissions.iter().map(i32::to_string).collect(),
            ));
        }
    }
    tags
}

/// The tags of a room's member list (kind 39002), with each member's roles.
pub fn members_tags(room_id: &str, members: &[RoomMember]) -> Vec<Vec<String>> {
    let mut tags = vec![
        domain::events::tag(&["-"]),
        domain::events::tag(&["d", room_id]),
    ];
    tags.extend(members.iter().map(|member| {
        let mut tag = vec!["p".to_string(), member.pubkey.clone()];
        tag.extend(member.roles.iter().cloned());
        tag
    }));
    tags
}
