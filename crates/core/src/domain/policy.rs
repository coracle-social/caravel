//! The rules a relay serves by, as pure functions over a snapshot of its settings,
//! access lists and rooms. The relay runtime keeps one snapshot per relay in
//! memory, so no rule reads the database, and stored queries and live delivery ask
//! the same functions.

use coracle_lib::events::Event;
use coracle_lib::kinds::is_ephemeral;

use crate::domain;
use crate::models::events::{EventGroup, EventHead};
use crate::models::kinds;
use crate::models::policy::{CallRefusal, EventFacts, Join, Refusal, RelaySnapshot};
use crate::models::rooms;

/// How far ahead of the relay's clock an event may be dated.
pub const CREATED_AT_UPPER_LIMIT: i64 = 300;

impl<'a> EventFacts<'a> {
    pub fn of(event: &'a Event, pubkey: &'a str) -> Self {
        Self {
            kind: event.kind,
            pubkey,
            room: domain::rooms::room_id(event),
        }
    }

    pub fn of_head(head: &'a EventHead, pubkey: &'a str) -> Self {
        Self {
            kind: head.kind,
            pubkey,
            room: head
                .tag(domain::rooms::room_tag(head.kind))
                .filter(|room| !room.is_empty()),
        }
    }

    pub fn of_group(group: &'a EventGroup, pubkey: &'a str) -> Self {
        let room = match domain::rooms::room_tag(group.kind) {
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

/// Kinds a client may never publish: the relay's own membership records, which it
/// writes itself, and Blossom authorizations, which only the media server takes.
pub fn is_relay_written(kind: u16) -> bool {
    use kinds::*;
    matches!(
        kind,
        RELAY_ADD_MEMBER | RELAY_REMOVE_MEMBER | RELAY_MEMBERS | RELAY_ROLE | BLOSSOM_AUTH
    )
}

/// Kinds that are never served: invite codes are credentials, and the others are
/// requests the relay acts on rather than keeps for readers.
pub const UNREADABLE_KINDS: [u16; 6] = [
    kinds::RELAY_INVITE,
    rooms::kinds::CREATE_INVITE,
    kinds::RELAY_JOIN,
    kinds::RELAY_LEAVE,
    kinds::PUSH_SUBSCRIPTION,
    kinds::BLOSSOM_AUTH,
];

/// The checks that read only what an unverified event already carries, run before
/// paying for its signature.
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

/// Whether a verified event may be published by a connection authenticated as
/// `authed`. Join requests pass here to be judged by their own rules, and a push
/// subscription is admitted as a subscription would be.
pub fn admit_write(
    snapshot: &RelaySnapshot,
    event: &Event,
    authed: &[String],
    now: i64,
) -> Result<(), Refusal> {
    check_protected(event, authed)?;
    screen(event, now)?;

    let author = event.pubkey.to_hex();

    if snapshot.banned_events.contains(&event.id.to_hex()) {
        return Err(Refusal::blocked(
            "this event has been banned from this relay",
        ));
    }

    if snapshot.is_banned(&author) {
        return Err(Refusal::blocked(
            "event author has been banned from this relay",
        ));
    }

    // A zap receipt or gift wrap for a member is welcome from anyone, if its room takes it.
    if !is_for_member(snapshot, event) {
        if authed.is_empty() {
            return Err(Refusal::AuthRequired(
                "authentication is required for access",
            ));
        }

        if !authed.contains(&author) && author != snapshot.relay_pubkey {
            return Err(Refusal::restricted(
                "you cannot publish events on behalf of others",
            ));
        }

        if event.kind == kinds::RELAY_JOIN {
            return Ok(());
        }

        if event.kind == kinds::PUSH_SUBSCRIPTION {
            return admit_req(snapshot, &[author]);
        }

        if !snapshot.public_write && !snapshot.has_access(&author) {
            return Err(Refusal::restricted("you are not a member of this relay"));
        }
    }

    // A room tag naming no room still claims one, so it takes the room checks.
    if domain::rooms::is_room_event(
        event.kind,
        event.tags.value(domain::rooms::room_tag(event.kind)),
    ) {
        check_room_write(snapshot, event)?;
    }

    Ok(())
}

/// NIP-70: a protected event may only be published by its author, authenticated,
/// and a repost may not carry one.
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

fn is_for_member(snapshot: &RelaySnapshot, event: &Event) -> bool {
    matches!(event.kind, kinds::ZAP_RECEIPT | kinds::GIFT_WRAP)
        && event
            .tags
            .find_all("p")
            .any(|tag| snapshot.has_access(tag.value()))
}

/// Whether an event may be published into the room machinery: rooms must be on,
/// the event must name its room, the room must exist (unless this creates it) and
/// be visible to the author, only moderators may change it, and only its members
/// may post in a restricted one.
pub fn check_room_write(snapshot: &RelaySnapshot, event: &Event) -> Result<(), Refusal> {
    if !snapshot.groups_enabled {
        return Err(Refusal::invalid("groups are not enabled"));
    }

    if domain::rooms::is_metadata_kind(event.kind) {
        return Err(Refusal::invalid("group metadata cannot be set directly"));
    }

    let Some(room_id) = domain::rooms::room_id(event) else {
        return Err(Refusal::invalid("missing group id"));
    };
    let room = snapshot.rooms.get(room_id);
    let author = event.pubkey.to_hex();

    if event.kind == rooms::kinds::CREATE_GROUP {
        if room.is_some() {
            return Err(Refusal::invalid("that group already exists"));
        }
    } else if room.is_none() {
        return Err(Refusal::invalid("group not found"));
    }

    if domain::rooms::is_moderation_kind(event.kind)
        && !snapshot.can_moderate_room(&author, room_id, event.kind)
    {
        return Err(Refusal::restricted(
            "you are not authorized to manage groups",
        ));
    }

    if matches!(
        event.kind,
        rooms::kinds::ASSIGN_PERMISSION | rooms::kinds::UNASSIGN_PERMISSION
    ) {
        for grant in event.tags.find_all("p") {
            let Some(kinds) = domain::rooms::permission_kinds(grant.as_slice()) else {
                return Err(Refusal::invalid("permissions are group management kinds"));
            };
            // Nobody hands out a power they don't hold.
            if !snapshot.can_manage(&author)
                && !kinds
                    .iter()
                    .all(|kind| snapshot.room_permits(&author, room_id, *kind))
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
    let has_access = snapshot.has_room_access(room, &author);
    let invited = event
        .tags
        .value("code")
        .is_some_and(|code| room.invites.contains(code));

    if room.hidden && !has_access && !invited {
        return Err(Refusal::invalid("group not found"));
    }

    match event.kind {
        rooms::kinds::JOIN_REQUEST if room.members.contains(&author) => {
            Err(Refusal::Unchanged("already a member".to_string()))
        }
        rooms::kinds::LEAVE_REQUEST if !room.members.contains(&author) => {
            Err(Refusal::Unchanged("not currently a member".to_string()))
        }
        rooms::kinds::JOIN_REQUEST | rooms::kinds::LEAVE_REQUEST => Ok(()),
        _ if room.restricted && !has_access => {
            Err(Refusal::restricted("you are not a member of that group"))
        }
        _ => Ok(()),
    }
}

/// How a NIP-43 join request is admitted, if it is. A new member must fit within
/// the plan's member limit.
pub fn admit_join(snapshot: &RelaySnapshot, event: &Event) -> Result<Join, Refusal> {
    let author = event.pubkey.to_hex();

    if snapshot.members.contains(&author) {
        return Ok(Join::AlreadyMember);
    }

    if snapshot.is_banned(&author) {
        return Err(Refusal::blocked("you have been banned from this relay"));
    }

    if snapshot
        .member_limit
        .is_some_and(|limit| snapshot.members.len() >= limit)
    {
        return Err(Refusal::restricted(
            "this relay has reached its member limit",
        ));
    }

    if snapshot.public_join {
        return Ok(Join::Open);
    }

    match event.tags.value("claim") {
        Some(claim) => Ok(Join::Claim(claim.to_string())),
        None if snapshot.charges_members => {
            Err(Refusal::restricted("this space requires a paid membership"))
        }
        None => Err(Refusal::restricted("no claim tag")),
    }
}

/// Whether a connection authenticated as `authed` may open a subscription.
pub fn admit_req(snapshot: &RelaySnapshot, authed: &[String]) -> Result<(), Refusal> {
    if authed.is_empty() {
        // An anonymous reader is admitted and then sees only what `can_receive` allows.
        if snapshot.anonymous_read {
            return Ok(());
        }
        return Err(Refusal::AuthRequired(
            "authentication is required for access",
        ));
    }

    if snapshot.public_read {
        if authed.iter().any(|pubkey| snapshot.is_banned(pubkey)) {
            return Err(Refusal::blocked("you have been banned from this relay"));
        }
    } else if !authed.iter().any(|pubkey| snapshot.has_access(pubkey)) {
        return Err(Refusal::restricted("you are not a member of this relay"));
    }

    Ok(())
}

/// Whether a reader may see an event: the one rule stored queries and live
/// delivery share. `None` is a reader who never authenticated, an outsider to
/// every room.
pub fn can_receive(snapshot: &RelaySnapshot, reader: Option<&str>, event: EventFacts<'_>) -> bool {
    if reader.is_none() && !snapshot.anonymous_read {
        return false;
    }

    // A push subscription isn't readable, but its author gets their own back.
    if event.kind == kinds::PUSH_SUBSCRIPTION {
        return reader == Some(event.pubkey);
    }

    if UNREADABLE_KINDS.contains(&event.kind) {
        return false;
    }

    if reader.is_some_and(|reader| snapshot.can_manage(reader)) {
        return true;
    }

    if snapshot.groups_enabled && domain::rooms::is_room_event(event.kind, event.room) {
        return can_read_room(snapshot, reader, event);
    }

    true
}

/// Rooms decide visibility for their own events: a room nobody created shows only
/// its deletion, a hidden room shows nothing to outsiders, and a private one shows
/// outsiders only its metadata.
fn can_read_room(snapshot: &RelaySnapshot, reader: Option<&str>, event: EventFacts<'_>) -> bool {
    let Some(room) = event.room.and_then(|room| snapshot.rooms.get(room)) else {
        return event.kind == rooms::kinds::DELETE_GROUP;
    };
    let has_access = reader.is_some_and(|reader| room.members.contains(reader));

    if room.hidden && !has_access {
        return false;
    }

    if matches!(
        event.kind,
        rooms::kinds::METADATA | rooms::kinds::DELETE_GROUP
    ) {
        return true;
    }

    !room.private || has_access
}

/// Whether any of a connection's identities may see an event.
pub fn can_any_receive(snapshot: &RelaySnapshot, authed: &[String], event: EventFacts<'_>) -> bool {
    if authed.is_empty() {
        return can_receive(snapshot, None, event);
    }
    authed
        .iter()
        .any(|reader| can_receive(snapshot, Some(reader), event))
}

/// How many of the counted events any of a connection's identities may see (NIP-45),
/// by the rule that decides what a subscription is sent.
pub fn count_visible(snapshot: &RelaySnapshot, authed: &[String], groups: &[EventGroup]) -> u64 {
    groups
        .iter()
        .filter(|group| {
            let author = hex::encode(group.pubkey);
            can_any_receive(snapshot, authed, EventFacts::of_group(group, &author))
        })
        .map(|group| group.count.unsigned_abs())
        .sum()
}

/// Whether a live event goes to a reader subscribed as `authed`: one banned or
/// removed since subscribing stops receiving at once.
pub fn can_deliver(snapshot: &RelaySnapshot, authed: &[String], event: EventFacts<'_>) -> bool {
    admit_req(snapshot, authed).is_ok() && can_any_receive(snapshot, authed, event)
}

/// Whether a pubkey may join a room's call: a member of the relay, in a room with
/// calls, and in the room itself if it is private or restricted.
pub fn admit_call(
    snapshot: &RelaySnapshot,
    pubkey: &str,
    room_id: &str,
) -> Result<(), CallRefusal> {
    if !snapshot.has_access(pubkey) {
        return Err(CallRefusal::NotMember);
    }
    let room = snapshot.rooms.get(room_id).ok_or(CallRefusal::NoRoom)?;
    if !room.livekit {
        return Err(CallRefusal::NoCalls);
    }
    if (room.private || room.restricted) && !snapshot.has_room_access(room, pubkey) {
        return Err(CallRefusal::NotRoomMember);
    }
    Ok(())
}

/// Whether events go to these readers without signatures: on a relay that strips
/// them, for anyone who doesn't manage it.
pub fn strips_signature(snapshot: &RelaySnapshot, authed: &[String]) -> bool {
    snapshot.strip_signatures && !authed.iter().any(|reader| snapshot.can_manage(reader))
}

/// Whether the relay keeps an event or only passes it on.
pub fn is_stored(kind: u16) -> bool {
    !is_ephemeral(kind)
}

/// The most push subscriptions one pubkey may hold on a relay.
pub const MAX_PUSH_SUBSCRIPTIONS: usize = 10;

/// Whether a push subscription is well formed, addressed to this relay by one of
/// its `hosts`, and within the limit given the author's `held` other subscriptions.
/// Whether its callback may be reached is the outbound client's to say.
pub fn check_push_subscription(
    event: &Event,
    hosts: &[String],
    held: usize,
) -> Result<(), Refusal> {
    if event.tags.value("d").is_none_or(str::is_empty) {
        return Err(Refusal::invalid("missing or empty d tag"));
    }

    let relay = event
        .tags
        .value("relay")
        .ok_or_else(|| Refusal::invalid("missing relay tag"))?;
    if !hosts.contains(&domain::relays::url_host(relay)) {
        return Err(Refusal::invalid(
            "relay tag does not match this relay's URL",
        ));
    }

    let filters: Vec<&str> = event.tags.values("filter").collect();
    if filters.is_empty() {
        return Err(Refusal::invalid("at least one filter tag is required"));
    }
    for filter in filters
        .iter()
        .chain(event.tags.values("ignore").collect::<Vec<_>>().iter())
    {
        serde_json::from_str::<coracle_lib::filters::Filter>(filter).map_err(|error| {
            Refusal::invalid(format!("filter tag contains invalid JSON: {error}"))
        })?;
    }

    let callbacks: Vec<&str> = event.tags.values("callback").collect();
    let callback = match callbacks.as_slice() {
        [] => return Err(Refusal::invalid("missing callback tag")),
        [callback] if !callback.is_empty() => *callback,
        [_] => return Err(Refusal::invalid("empty callback tag")),
        _ => return Err(Refusal::invalid("too many callback tags")),
    };

    let url: url::Url = callback
        .parse()
        .map_err(|_| Refusal::invalid("callback must be a valid HTTP or HTTPS URL"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Refusal::invalid(
            "callback must be a valid HTTP or HTTPS URL",
        ));
    }

    if held >= MAX_PUSH_SUBSCRIPTIONS {
        return Err(Refusal::invalid("too many subscriptions registered"));
    }

    Ok(())
}
