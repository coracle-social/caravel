//! Writes: each makes one change on a caller's connection, at a time the caller
//! gives, including the events the relay signs to publish it, which it returns for
//! the caller to deliver once the transaction commits. Commands never read the
//! clock, publish on the bus or call an adapter beyond signing with the caller's key.

pub mod activity;
pub mod bans;
pub mod billing;
pub mod calls;
pub mod claims;
pub mod events;
pub mod management;
pub mod media;
pub mod memberships;
pub mod notifications;
pub mod relays;
pub mod roles;
pub mod rooms;
pub mod sessions;
pub mod tenants;
pub mod usage;
