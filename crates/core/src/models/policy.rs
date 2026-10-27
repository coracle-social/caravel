//! What a relay's rules read and how they answer: a snapshot of its settings, access
//! lists and rooms, the facts about an event the rules weigh, and the refusals they
//! give.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::models::methods::{INVITE_METHODS, MANAGED_METHODS};
use crate::models::relays::Relay;

/// What a relay's rules read: its settings, who holds which access, and its rooms,
/// as of the last change to any of them.
#[derive(Clone, Debug, Default)]
pub struct RelaySnapshot {
    pub relay_id: String,
    /// The relay's own pubkey, which signs what it publishes about itself.
    pub relay_pubkey: String,
    /// The tenant who owns the relay.
    pub tenant_pubkey: String,
    pub public_read: bool,
    pub anonymous_read: bool,
    pub public_write: bool,
    pub public_join: bool,
    pub strip_signatures: bool,
    pub groups_enabled: bool,
    pub members_can_invite: bool,
    /// The most members the relay's plan allows, if it has a limit.
    pub member_limit: Option<usize>,
    /// The relay sells memberships, so joining needs a claim.
    pub charges_members: bool,
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
    /// Only its members and the relay's managers may read it or listen to its calls.
    pub private: bool,
    /// Join requests without one of its invite codes are ignored.
    pub closed: bool,
    /// The room has calls.
    pub livekit: bool,
    /// Only its members and the relay's managers may post to it or speak in its calls.
    pub restricted: bool,
    pub members: HashSet<String>,
    /// The moderation kinds members may publish in the room.
    pub permissions: HashMap<String, BTreeSet<u16>>,
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
            relay_pubkey: relay.pubkey.clone(),
            tenant_pubkey: relay.tenant_pubkey.clone(),
            public_read: relay.policy_public_read,
            anonymous_read: relay.policy_anonymous_read,
            public_write: relay.policy_public_write,
            public_join: relay.policy_public_join,
            strip_signatures: relay.policy_strip_signatures,
            groups_enabled: relay.groups_enabled,
            members_can_invite: relay.members_can_invite,
            charges_members: relay.charges_members(),
            member_limit: relay
                .member_limit()
                .and_then(|limit| usize::try_from(limit).ok()),
            ..Default::default()
        }
    }

    /// The owner and the relay itself, who may do anything.
    pub fn can_manage(&self, pubkey: &str) -> bool {
        pubkey == self.tenant_pubkey || pubkey == self.relay_pubkey
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

    /// Whether a pubkey may publish a moderation `kind` in a room: the relay's
    /// managers may publish any, and the room's members those they're permitted.
    pub fn can_moderate_room(&self, pubkey: &str, room_id: &str, kind: u16) -> bool {
        self.can_manage(pubkey) || self.room_permits(pubkey, room_id, kind)
    }

    pub fn room_permits(&self, pubkey: &str, room_id: &str, kind: u16) -> bool {
        self.rooms
            .get(room_id)
            .and_then(|room| room.permissions.get(pubkey))
            .is_some_and(|permissions| permissions.contains(&kind))
    }

    pub fn has_room_access(&self, room: &RoomSnapshot, pubkey: &str) -> bool {
        self.can_manage(pubkey) || room.members.contains(pubkey)
    }
}

/// Why a pubkey may not join a room's call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallRefusal {
    NotMember,
    NoRoom,
    /// The room has no calls.
    NoCalls,
    /// The room keeps its calls to its members.
    NotRoomMember,
}
