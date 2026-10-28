//! Event kinds with a meaning on these relays, and the NIP-29 room kinds.

pub const DELETION: u16 = 5;

pub const REPOST: u16 = 6;

pub const GENERIC_REPOST: u16 = 16;

pub const VANISH: u16 = 62;

pub const GIFT_WRAP: u16 = 1059;

pub const RELAY_ADD_MEMBER: u16 = 8000;

pub const RELAY_REMOVE_MEMBER: u16 = 8001;

pub const ZAP_RECEIPT: u16 = 9735;

pub const RELAY_LIST: u16 = 10002;

pub const ROOM_LIST: u16 = 10009;

pub const RELAY_MEMBERS: u16 = 13534;

pub const CLIENT_AUTH: u16 = 22242;

pub const BLOSSOM_AUTH: u16 = 24242;

pub const HTTP_AUTH: u16 = 27235;

pub const RELAY_JOIN: u16 = 28934;

pub const RELAY_INVITE: u16 = 28935;

pub const RELAY_LEAVE: u16 = 28936;

pub const PUSH_SUBSCRIPTION: u16 = 30390;

pub const RELAY_ROLE: u16 = 33534;

/// The NIP-29 room kinds.
pub mod rooms {
    pub const PUT_USER: u16 = 9000;
    pub const REMOVE_USER: u16 = 9001;
    pub const EDIT_METADATA: u16 = 9002;
    pub const DELETE_EVENT: u16 = 9005;
    pub const CREATE_GROUP: u16 = 9007;
    pub const DELETE_GROUP: u16 = 9008;
    pub const CREATE_INVITE: u16 = 9009;
    pub const PUT_PINS: u16 = 9010;
    pub const ASSIGN_PERMISSION: u16 = 9011;
    pub const UNASSIGN_PERMISSION: u16 = 9012;
    pub const JOIN_REQUEST: u16 = 9021;
    pub const LEAVE_REQUEST: u16 = 9022;
    pub const METADATA: u16 = 39000;
    pub const ADMINS: u16 = 39001;
    pub const MEMBERS: u16 = 39002;
    pub const ROLES: u16 = 39003;
    pub const LIVEKIT_PARTICIPANTS: u16 = 39004;
    pub const PINS: u16 = 39005;
}
