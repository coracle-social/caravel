//! The facts about an event the rules weigh.

use coracle_lib::events::Event;

use crate::events::{EventGroup, EventHead};
use crate::policy::EventFacts;

impl<'a> EventFacts<'a> {
    pub fn of(event: &'a Event, pubkey: &'a str) -> Self {
        Self {
            kind: event.kind,
            pubkey,
            room: crate::rooms::room_id(event),
        }
    }

    pub fn of_head(head: &'a EventHead, pubkey: &'a str) -> Self {
        Self {
            kind: head.kind,
            pubkey,
            room: head
                .tag(crate::rooms::room_tag(head.kind))
                .filter(|room| !room.is_empty()),
        }
    }

    pub fn of_group(group: &'a EventGroup, pubkey: &'a str) -> Self {
        let room = match crate::rooms::room_tag(group.kind) {
            "d" => group.d.as_deref(),
            _ => group.h.as_deref(),
        };
        Self {
            kind: group.kind,
            pubkey,
            room: room.filter(|room| !room.is_empty()),
        }
    }
}
