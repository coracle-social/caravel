//! Whether a verified event may be published, and the checks before its signature.

use coracle_lib::events::Event;

use crate as domain;
use crate::policy::{Refusal, RelaySnapshot};
use caravel_util::nostr::kinds;

pub const CREATED_AT_UPPER_LIMIT: i64 = 300;

pub fn is_relay_written(kind: u16) -> bool {
    use kinds::*;
    matches!(
        kind,
        RELAY_ADD_MEMBER | RELAY_REMOVE_MEMBER | RELAY_MEMBERS | RELAY_ROLE | BLOSSOM_AUTH
    )
}

pub const UNREADABLE_KINDS: [u16; 6] = [
    kinds::RELAY_INVITE,
    kinds::rooms::CREATE_INVITE,
    kinds::RELAY_JOIN,
    kinds::RELAY_LEAVE,
    kinds::PUSH_SUBSCRIPTION,
    kinds::BLOSSOM_AUTH,
];

pub fn screen(event: &Event, now: i64) -> Result<(), Refusal> {
    if is_relay_written(event.kind) {
        return Err(Refusal::invalid("this event's kind is not accepted"));
    }

    if event.created_at - now > CREATED_AT_UPPER_LIMIT {
        return Err(Refusal::invalid(
            "this event is dated too far in the future",
        ));
    }

    Ok(())
}

fn check_protected(event: &Event, authed: &[String]) -> Result<(), Refusal> {
    if coracle_lib::protected::is_protected(&event.tags) {
        if authed.is_empty() {
            return Err(Refusal::AuthRequired(
                "must be published by authenticated event author",
            ));
        }
        if !authed.contains(&event.pubkey.to_hex()) {
            return Err(Refusal::blocked("must be published by event author"));
        }
        return Ok(());
    }

    if matches!(event.kind, kinds::REPOST | kinds::GENERIC_REPOST)
        && let Ok(reposted) = serde_json::from_str::<serde_json::Value>(&event.content)
        && reposted["tags"].as_array().is_some_and(|tags| {
            tags.iter()
                .any(|tag| tag.get(0).and_then(|name| name.as_str()) == Some("-"))
        })
    {
        return Err(Refusal::blocked("can't repost nip70 protected"));
    }

    Ok(())
}

impl RelaySnapshot {
    pub fn admit_write(&self, event: &Event, authed: &[String], now: i64) -> Result<(), Refusal> {
        check_protected(event, authed)?;
        screen(event, now)?;

        let author = event.pubkey.to_hex();

        if self.banned_events.contains(&event.id.to_hex()) {
            return Err(Refusal::blocked(
                "this event has been banned from this relay",
            ));
        }

        if self.is_banned(&author) {
            return Err(Refusal::blocked(
                "event author has been banned from this relay",
            ));
        }

        // A zap receipt or gift wrap for a member is welcome from anyone, if its room takes it.
        if !self.is_for_member(event) {
            if authed.is_empty() {
                return Err(Refusal::AuthRequired(
                    "authentication is required for access",
                ));
            }

            if !authed.contains(&author) && author != self.relay_pubkey {
                return Err(Refusal::restricted(
                    "you cannot publish events on behalf of others",
                ));
            }

            if event.kind == kinds::RELAY_JOIN {
                return Ok(());
            }

            if event.kind == kinds::PUSH_SUBSCRIPTION {
                return self.admit_req(&[author]);
            }

            if !self.public_write && !self.has_access(&author) {
                return Err(Refusal::restricted("you are not a member of this relay"));
            }
        }

        // A room tag naming no room still claims one, so it takes the room checks.
        if domain::rooms::is_room_event(
            event.kind,
            event.tags.value(domain::rooms::room_tag(event.kind)),
        ) {
            self.check_room_write(event)?;
        }

        Ok(())
    }
    fn is_for_member(&self, event: &Event) -> bool {
        matches!(event.kind, kinds::ZAP_RECEIPT | kinds::GIFT_WRAP)
            && event
                .tags
                .find_all("p")
                .any(|tag| self.has_access(tag.value()))
    }
}
