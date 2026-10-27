//! Event kinds with a meaning on these relays, other than the NIP-29 room kinds,
//! which `rooms` owns.

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
