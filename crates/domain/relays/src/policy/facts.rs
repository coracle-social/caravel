//! The facts about an event the rules weigh.

use coracle_lib::events::Event;

use crate::events::{EventGroup, EventHead};
use crate::policy::EventFacts;

impl<'a> EventFacts<'a> {
    pub fn of(event: &'a Event) -> Self {
        Self {
            kind: event.kind,
            pubkey: event.pubkey,
            room: crate::rooms::room_id(event),
            created_at: Some(event.created_at),
        }
    }

    pub fn of_head(head: &'a EventHead) -> Self {
        Self {
            kind: head.kind,
            pubkey: head.pubkey,
            room: head
                .tag(crate::rooms::room_tag(head.kind))
                .filter(|room| !room.is_empty()),
            created_at: Some(head.created_at),
        }
    }

    pub fn of_group(group: &'a EventGroup) -> Self {
        let room = match crate::rooms::room_tag(group.kind) {
            "d" => group.d.as_deref(),
            _ => group.h.as_deref(),
        };
        Self {
            kind: group.kind,
            pubkey: group.pubkey,
            room: room.filter(|room| !room.is_empty()),
            created_at: None,
        }
    }
}
