//! Who may see an event, and who may subscribe.

use coracle_lib::keys::PublicKey;

use crate::events::EventGroup;
use crate::policy::write::UNREADABLE_KINDS;
use crate::policy::{EventFacts, Refusal, RelaySnapshot};
use caravel_util::nostr::kinds;

impl RelaySnapshot {
    pub fn admit_req(&self, authed: &[PublicKey]) -> Result<(), Refusal> {
        if authed.is_empty() {
            // An anonymous reader is admitted and then sees only what `can_receive` allows.
            if self.anonymous_read {
                return Ok(());
            }
            return Err(Refusal::AuthRequired(
                "authentication is required for access",
            ));
        }

        if self.public_read {
            if authed.iter().any(|pubkey| self.is_banned(pubkey)) {
                return Err(Refusal::blocked("you have been banned from this relay"));
            }
        } else if !authed.iter().any(|pubkey| self.has_access(pubkey)) {
            return Err(Refusal::restricted("you are not a member of this relay"));
        }

        Ok(())
    }
    pub fn can_receive(&self, reader: Option<&PublicKey>, event: EventFacts<'_>) -> bool {
        if reader.is_none() && !self.anonymous_read {
            return false;
        }

        // A push subscription isn't readable, but its author gets their own back.
        if event.kind == kinds::PUSH_SUBSCRIPTION {
            return reader == Some(&event.pubkey);
        }

        if UNREADABLE_KINDS.contains(&event.kind) {
            return false;
        }

        if reader.is_some_and(|reader| self.can_manage(reader)) {
            return true;
        }

        if self.groups_enabled && crate::rooms::is_room_event(event.kind, event.room) {
            return self.can_read_room(reader, event);
        }

        true
    }
    fn can_read_room(&self, reader: Option<&PublicKey>, event: EventFacts<'_>) -> bool {
        let Some(room) = event.room.and_then(|room| self.rooms.get(room)) else {
            return event.kind == kinds::rooms::DELETE_GROUP;
        };
        let has_access = reader.is_some_and(|reader| room.members.contains(reader));

        if room.hidden && !has_access {
            return false;
        }

        if matches!(
            event.kind,
            kinds::rooms::METADATA | kinds::rooms::DELETE_GROUP
        ) {
            return true;
        }

        !room.private || has_access
    }
    pub fn can_any_receive(&self, authed: &[PublicKey], event: EventFacts<'_>) -> bool {
        if authed.is_empty() {
            return self.can_receive(None, event);
        }
        authed
            .iter()
            .any(|reader| self.can_receive(Some(reader), event))
    }
    pub fn count_visible(&self, authed: &[PublicKey], groups: &[EventGroup]) -> u64 {
        groups
            .iter()
            .filter(|group| self.can_any_receive(authed, EventFacts::of_group(group)))
            .map(|group| group.count)
            .sum()
    }
    pub fn can_deliver(&self, authed: &[PublicKey], event: EventFacts<'_>) -> bool {
        self.admit_req(authed).is_ok() && self.can_any_receive(authed, event)
    }
}
