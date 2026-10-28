//! NIP-86 calls, decoded from their method and params, and what carrying one out
//! did.

use coracle_lib::events::Event;
use serde_json::{Value, json};

use crate::roles::Role;

/// A NIP-86 call, decoded from its method and params.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    ChangeRelayName(String),
    ChangeRelayDescription(String),
    ChangeRelayIcon(String),
    BanPubkey(String, String),
    UnbanPubkey(String),
    AllowPubkey(String),
    UnallowPubkey(String),
    ListBannedPubkeys,
    ListAllowedPubkeys,
    BanEvent(String, String),
    AllowEvent(String),
    ListBannedEvents,
    CreateRole(Role),
    EditRole(Role),
    DeleteRole(String),
    AssignRole(String, String),
    UnassignRole(String, String),
    AssignMethod(String, String),
    UnassignMethod(String, String),
    ListMethodAssignees,
    ListClaims,
    CreateClaim(String),
    DeleteClaim(String),
}

/// What a call did.
pub enum Outcome {
    /// It answered without touching the relay's access lists or rooms.
    Answer(Value),
    /// It changed the relay's access lists or rooms, publishing these events about
    /// it, for the caller to reload the relay and deliver them.
    AccessChanged(Vec<Event>),
}

impl Outcome {
    /// What the call answers with.
    pub fn result(&self) -> Value {
        match self {
            Outcome::Answer(result) => result.clone(),
            Outcome::AccessChanged(_) => json!(true),
        }
    }
}
