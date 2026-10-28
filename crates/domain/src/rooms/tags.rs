//! The tags of the events a relay publishes about a room's access.

use crate as domain;
use crate::rooms::{MODERATION_KINDS, RoomMember};

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
