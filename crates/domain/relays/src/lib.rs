//! The relay itself: the models, utilities, queries and commands that correctly
//! implement a nostr relay. Nothing here knows about billing or the layers above.

pub mod access;
pub mod calls;
pub mod events;
pub mod management;
pub mod media;
pub mod notify;
pub mod policy;
pub mod profiles;
pub mod rooms;
pub mod transfer;
