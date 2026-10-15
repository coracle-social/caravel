//! The rules a relay serves by, as pure functions over a snapshot of its settings,
//! access lists and rooms. The relay runtime keeps one snapshot per relay in
//! memory, so no rule reads the database, and stored queries and live delivery ask
//! the same functions.

use std::collections::{BTreeSet, HashMap, HashSet};

use coracle_lib::events::Event;
use coracle_lib::kinds::is_ephemeral;
use sqlx::PgExecutor;

use crate::access::{self, INVITE_METHODS, MANAGED_METHODS};
use crate::error::Result;
use crate::events::EventHead;
use crate::plans;
use crate::relays::Relay;
use crate::rooms::{self, ADMIN_ROLE};

/// Kinds with a meaning on these relays.
pub mod kinds {
    pub const ZAP_RECEIPT: u16 = 9735;
    pub const GIFT_WRAP: u16 = 1059;
    pub const REPOST: u16 = 6;
    pub const GENERIC_REPOST: u16 = 16;
    pub const RELAY_ADD_MEMBER: u16 = 8000;
    pub const RELAY_REMOVE_MEMBER: u16 = 8001;
    pub const RELAY_MEMBERS: u16 = 13534;
    pub const BLOB_DESCRIPTOR: u16 = 24242;
    pub const RELAY_JOIN: u16 = 28934;
    pub const RELAY_INVITE: u16 = 28935;
    pub const RELAY_LEAVE: u16 = 28936;
    pub const PUSH_SUBSCRIPTION: u16 = 30390;
    pub const RELAY_ROLE: u16 = 33534;
}

/// How far ahead of the relay's clock an event may be dated.
pub const CREATED_AT_UPPER_LIMIT: i64 = 300;

/// What a relay's rules read: its settings, who holds which access, and its rooms,
/// as of the last change to any of them.
#[derive(Clone, Debug, Default)]
pub struct RelaySnapshot {
    pub relay_id: String,
    /// The relay's own pubkey, which signs what it publishes about itself.
    pub self_pubkey: String,
    /// The tenant who owns the relay.
    pub owner: String,
    pub public_read: bool,
    pub anonymous_read: bool,
    pub public_write: bool,
    pub public_join: bool,
    pub strip_signatures: bool,
    pub groups_enabled: bool,
    pub members_can_invite: bool,
    /// The most members the relay's plan allows, if it has a limit.
    pub member_limit: Option<usize>,
    pub members: HashSet<String>,
    /// NIP-86 methods granted to pubkeys individually.
    pub method_grants: HashMap<String, BTreeSet<String>>,
    pub banned_pubkeys: HashSet<String>,
    pub banned_events: HashSet<String>,
    pub rooms: HashMap<String, RoomSnapshot>,
}

/// What the rules read about one room.
#[derive(Clone, Debug, Default)]
pub struct RoomSnapshot {
    /// Only its members and the relay's managers know it exists.
    pub hidden: bool,
    /// Only its members and the relay's managers may read it.
    pub private: bool,
    /// Only its members and the relay's managers may post, and joining needs an invite.
    pub closed: bool,
    pub members: HashSet<String>,
    /// Members holding the room's admin role, who may moderate it.
    pub admins: HashSet<String>,
    pub invites: HashSet<String>,
}

/// The facts about an event the rules read, whether it is stored or just arrived.
#[derive(Clone, Copy, Debug)]
pub struct EventFacts<'a> {
    pub kind: u16,
    pub pubkey: &'a str,
    /// The room the event is about or posted in.
    pub room: Option<&'a str>,
}

/// How a join request is admitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Join {
    /// The pubkey is already a member.
    AlreadyMember,
    /// The relay takes anyone.
    Open,
    /// The request presents a claim, which still has to be live.
    Claim(String),
}

/// Why the relay said no, with the machine-readable prefix NIP-01 asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The client has to prove who it is first.
    AuthRequired(&'static str),
    /// The relay doesn't take that, whoever is asking.
    Blocked(String),
    /// A reconciliation that isn't open.
    Closed(&'static str),
    /// Already here, which NIP-01 counts as acceptance.
    Duplicate,
    /// A request for what is already so, such as joining a room as its member.
    Unchanged(String),
    /// The relay's fault rather than the client's.
    Error(&'static str),
    /// Malformed, or asking for something incoherent.
    Invalid(String),
    /// More at once than the relay allows.
    RateLimited(String),
    /// Authenticated, but as somebody not allowed to do this.
    Restricted(String),
}

impl Refusal {
    pub fn blocked(reason: impl Into<String>) -> Self {
        Self::Blocked(reason.into())
    }

    pub fn invalid(reason: impl Into<String>) -> Self {
        Self::Invalid(reason.into())
    }

    pub fn restricted(reason: impl Into<String>) -> Self {
        Self::Restricted(reason.into())
    }

    pub fn rate_limited(reason: impl Into<String>) -> Self {
        Self::RateLimited(reason.into())
    }

    /// Whether an event refused this way is nevertheless on the relay.
    pub fn accepted(&self) -> bool {
        matches!(self, Self::Duplicate)
    }

    /// Whether answering this means offering an auth challenge alongside it.
    pub fn needs_auth(&self) -> bool {
        matches!(self, Self::AuthRequired(_))
    }

    /// The wire message, prefix included.
    pub fn message(&self) -> String {
        match self {
            Self::AuthRequired(reason) => format!("auth-required: {reason}"),
            Self::Blocked(reason) => format!("blocked: {reason}"),
            Self::Closed(reason) => format!("closed: {reason}"),
            Self::Duplicate => "duplicate: already have this event".to_string(),
            Self::Unchanged(reason) => format!("duplicate: {reason}"),
            Self::Error(reason) => format!("error: {reason}"),
            Self::Invalid(reason) => format!("invalid: {reason}"),
            Self::RateLimited(reason) => format!("rate-limited: {reason}"),
            Self::Restricted(reason) => format!("restricted: {reason}"),
        }
    }
}

impl RelaySnapshot {
    /// A snapshot of a relay's settings, before its access lists and rooms are added.
    pub fn of(relay: &Relay) -> Self {
        Self {
            relay_id: relay.id.clone(),
            self_pubkey: relay.pubkey.clone(),
            owner: relay.tenant_pubkey.clone(),
            public_read: relay.policy_public_read,
            anonymous_read: relay.policy_anonymous_read,
            public_write: relay.policy_public_write,
            public_join: relay.policy_public_join,
            strip_signatures: relay.policy_strip_signatures,
            groups_enabled: relay.groups_enabled,
            members_can_invite: relay.members_can_invite,
            member_limit: plans::get_plan(&relay.plan_id)
                .and_then(|plan| plan.members)
                .and_then(|limit| usize::try_from(limit).ok()),
            ..Default::default()
        }
    }

    /// The owner and the relay itself, who may do anything.
    pub fn can_manage(&self, pubkey: &str) -> bool {
        pubkey == self.owner || pubkey == self.self_pubkey
    }

    /// Whether a pubkey belongs on the relay: a manager or a member.
    pub fn has_access(&self, pubkey: &str) -> bool {
        self.can_manage(pubkey) || self.members.contains(pubkey)
    }

    pub fn is_banned(&self, pubkey: &str) -> bool {
        self.banned_pubkeys.contains(pubkey)
    }

    /// The NIP-86 methods a pubkey may call: every one for the relay's managers,
    /// otherwise those granted to it, and the invite methods for any member of a
    /// relay whose members may invite.
    pub fn methods_for(&self, pubkey: &str) -> Vec<String> {
        if self.can_manage(pubkey) {
            return MANAGED_METHODS
                .iter()
                .map(|method| method.to_string())
                .collect();
        }

        let mut methods: Vec<String> = self
            .method_grants
            .get(pubkey)
            .map(|methods| methods.iter().cloned().collect())
            .unwrap_or_default();

        if self.members_can_invite && self.members.contains(pubkey) {
            for method in INVITE_METHODS {
                if !methods.iter().any(|held| held == method) {
                    methods.push(method.to_string());
                }
            }
        }

        methods
    }

    /// Whether a pubkey may call a NIP-86 method. Anyone holding a method may ask
    /// which ones they hold.
    pub fn may_call(&self, pubkey: &str, method: &str) -> bool {
        let methods = self.methods_for(pubkey);
        if method == "supportedmethods" {
            !methods.is_empty()
        } else {
            methods.iter().any(|held| held == method)
        }
    }

    /// Whether a pubkey may moderate a room: the relay's managers, and the room's
    /// own admins.
    pub fn can_moderate_room(&self, pubkey: &str, room_id: &str) -> bool {
        self.can_manage(pubkey)
            || self
                .rooms
                .get(room_id)
                .is_some_and(|room| room.admins.contains(pubkey))
    }

    fn has_room_access(&self, room: &RoomSnapshot, pubkey: &str) -> bool {
        self.can_manage(pubkey) || room.members.contains(pubkey)
    }
}

impl<'a> EventFacts<'a> {
    pub fn of(event: &'a Event, pubkey: &'a str) -> Self {
        Self {
            kind: event.kind,
            pubkey,
            room: rooms::room_id(event),
        }
    }

    pub fn of_head(head: &'a EventHead, pubkey: &'a str) -> Self {
        let name = if rooms::is_metadata_kind(head.kind) {
            'd'
        } else {
            'h'
        };
        Self {
            kind: head.kind,
            pubkey,
            room: head.tag(name).filter(|room| !room.is_empty()),
        }
    }
}

/// Load a relay's snapshot: its settings, members, method grants, bans and rooms.
pub async fn load_snapshot(db: impl PgExecutor<'_> + Copy, relay: &Relay) -> Result<RelaySnapshot> {
    let mut snapshot = RelaySnapshot::of(relay);

    snapshot.members = access::list_members(db, &relay.id)
        .await?
        .into_iter()
        .map(|member| member.pubkey)
        .collect();
    snapshot.method_grants = access::list_method_grants(db, &relay.id)
        .await?
        .into_iter()
        .map(|(pubkey, methods)| (pubkey, methods.into_iter().collect()))
        .collect();
    snapshot.banned_pubkeys = access::list_banned_pubkeys(db, &relay.id)
        .await?
        .into_iter()
        .map(|ban| ban.subject)
        .collect();
    snapshot.banned_events = access::list_banned_events(db, &relay.id)
        .await?
        .into_iter()
        .map(|ban| ban.subject)
        .collect();

    for room in rooms::list_rooms(db, &relay.id).await? {
        let room_snapshot = RoomSnapshot {
            hidden: room.has_flag("hidden"),
            private: room.has_flag("private"),
            closed: room.has_flag("closed"),
            ..Default::default()
        };
        snapshot.rooms.insert(room.id, room_snapshot);
    }
    for member in rooms::list_room_members(db, &relay.id).await? {
        if let Some(room) = snapshot.rooms.get_mut(&member.room_id) {
            if member.roles.iter().any(|role| role == ADMIN_ROLE) {
                room.admins.insert(member.pubkey.clone());
            }
            room.members.insert(member.pubkey);
        }
    }
    for (room_id, code) in rooms::list_room_invites(db, &relay.id).await? {
        if let Some(room) = snapshot.rooms.get_mut(&room_id) {
            room.invites.insert(code);
        }
    }

    Ok(snapshot)
}

/// Kinds a client may never publish: the relay's own membership records, which it
/// writes itself, and blob descriptors, which only the media server takes.
fn is_relay_written(kind: u16) -> bool {
    use kinds::*;
    matches!(
        kind,
        RELAY_ADD_MEMBER | RELAY_REMOVE_MEMBER | RELAY_MEMBERS | RELAY_ROLE | BLOB_DESCRIPTOR
    )
}

/// Kinds that are never served: invite codes are credentials, and the others are
/// requests the relay acts on rather than keeps for readers.
fn is_unreadable(kind: u16) -> bool {
    use kinds::*;
    matches!(
        kind,
        RELAY_INVITE
            | rooms::kinds::CREATE_INVITE
            | RELAY_JOIN
            | RELAY_LEAVE
            | PUSH_SUBSCRIPTION
            | BLOB_DESCRIPTOR
    )
}

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
/// `authed`, in zooid's order. Join requests and push subscriptions pass here to be
/// judged by their own rules.
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

    // A zap receipt or gift wrap is for its recipient, so a member being addressed
    // is enough, whoever delivers it.
    if is_for_member(snapshot, event) {
        return Ok(());
    }

    if authed.is_empty() {
        return Err(Refusal::AuthRequired(
            "authentication is required for access",
        ));
    }

    if !authed.contains(&author) && author != snapshot.self_pubkey {
        return Err(Refusal::restricted(
            "you cannot publish events on behalf of others",
        ));
    }

    if matches!(event.kind, kinds::RELAY_JOIN | kinds::PUSH_SUBSCRIPTION) {
        return Ok(());
    }

    if !snapshot.public_write && !snapshot.has_access(&author) {
        return Err(Refusal::restricted("you are not a member of this relay"));
    }

    if rooms::is_room_event(event.kind, rooms::room_id(event)) {
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
/// the room must exist (unless this creates it) and be visible to the author, only
/// moderators may change it, and only its members may post in a closed one.
pub fn check_room_write(snapshot: &RelaySnapshot, event: &Event) -> Result<(), Refusal> {
    if !snapshot.groups_enabled {
        return Err(Refusal::invalid("groups are not enabled"));
    }

    if rooms::is_metadata_kind(event.kind) {
        return Err(Refusal::invalid("group metadata cannot be set directly"));
    }

    let room_id = rooms::room_id(event).unwrap_or_default();
    let room = snapshot.rooms.get(room_id);
    let author = event.pubkey.to_hex();

    if event.kind == rooms::kinds::CREATE_GROUP {
        if room.is_some() {
            return Err(Refusal::invalid("that group already exists"));
        }
    } else if room.is_none() {
        return Err(Refusal::invalid("group not found"));
    }

    if rooms::is_moderation_kind(event.kind) && !snapshot.can_moderate_room(&author, room_id) {
        return Err(Refusal::restricted(
            "you are not authorized to manage groups",
        ));
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
        _ if room.closed && !has_access => {
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

    if is_unreadable(event.kind) {
        return false;
    }

    if reader.is_some_and(|reader| snapshot.can_manage(reader)) {
        return true;
    }

    if snapshot.groups_enabled && rooms::is_room_event(event.kind, event.room) {
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

/// Whether events go to these readers without signatures: on a relay that strips
/// them, for anyone who doesn't manage it.
pub fn strips_signature(snapshot: &RelaySnapshot, authed: &[String]) -> bool {
    snapshot.strip_signatures && !authed.iter().any(|reader| snapshot.can_manage(reader))
}

/// Whether the relay keeps an event or only passes it on.
pub fn is_stored(kind: u16) -> bool {
    !is_ephemeral(kind)
}
