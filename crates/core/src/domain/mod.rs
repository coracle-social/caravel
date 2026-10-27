//! The rules: pure functions over models that decide what is allowed and compute
//! what follows from it. Nothing here touches the database, the clock or the
//! outside world.

pub mod auth;
pub mod billing;
pub mod calls;
pub mod domains;
pub mod events;
pub mod management;
pub mod media;
pub mod members;
pub mod memberships;
pub mod policy;
pub mod relays;
pub mod roles;
pub mod rooms;
pub mod transfer;
