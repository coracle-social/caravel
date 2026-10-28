//! Whether an event may be published into a room.

use coracle_lib::events::Event;

use crate as domain;
use crate::policy::{Refusal, RelaySnapshot};
use caravel_util::nostr::kinds;

impl RelaySnapshot {
    pub fn check_room_write(&self, event: &Event) -> Result<(), Refusal> {
        if !self.groups_enabled {
            return Err(Refusal::invalid("groups are not enabled"));
        }

        if domain::rooms::is_metadata_kind(event.kind) {
            return Err(Refusal::invalid("group metadata cannot be set directly"));
        }

        let Some(room_id) = domain::rooms::room_id(event) else {
            return Err(Refusal::invalid("missing group id"));
        };
        let room = self.rooms.get(room_id);
        let author = event.pubkey.to_hex();

        if event.kind == kinds::rooms::CREATE_GROUP {
            if room.is_some() {
                return Err(Refusal::invalid("that group already exists"));
            }
        } else if room.is_none() {
            return Err(Refusal::invalid("group not found"));
        }

        if domain::rooms::is_moderation_kind(event.kind)
            && !self.can_moderate_room(&author, room_id, event.kind)
        {
            return Err(Refusal::restricted(
                "you are not authorized to manage groups",
            ));
        }

        if matches!(
            event.kind,
            kinds::rooms::ASSIGN_PERMISSION | kinds::rooms::UNASSIGN_PERMISSION
        ) {
            for grant in event.tags.find_all("p") {
                let Some(kinds) = domain::rooms::permission_kinds(grant.as_slice()) else {
                    return Err(Refusal::invalid("permissions are group management kinds"));
                };
                // Nobody hands out a power they don't hold.
                if !self.can_manage(&author)
                    && !kinds
                        .iter()
                        .all(|kind| self.room_permits(&author, room_id, *kind))
                {
                    return Err(Refusal::restricted(
                        "you may only grant permissions you hold",
                    ));
                }
            }
        }

        let Some(room) = room else {
            return Ok(());
        };
        let has_access = self.has_room_access(room, &author);
        let invited = event
            .tags
            .value("code")
            .is_some_and(|code| room.invites.contains(code));

        if room.hidden && !has_access && !invited {
            return Err(Refusal::invalid("group not found"));
        }

        match event.kind {
            kinds::rooms::JOIN_REQUEST if room.members.contains(&author) => {
                Err(Refusal::Unchanged("already a member".to_string()))
            }
            kinds::rooms::LEAVE_REQUEST if !room.members.contains(&author) => {
                Err(Refusal::Unchanged("not currently a member".to_string()))
            }
            kinds::rooms::JOIN_REQUEST | kinds::rooms::LEAVE_REQUEST => Ok(()),
            _ if room.restricted && !has_access => {
                Err(Refusal::restricted("you are not a member of that group"))
            }
            _ => Ok(()),
        }
    }
}
