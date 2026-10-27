//! Writes: each makes one change on a caller's connection, at a time the caller
//! gives, including the events the relay signs to publish it, which it returns for
//! the caller to deliver once the transaction commits. Commands never read the
//! clock, publish on the bus or call adapters.

pub mod activity;
pub mod bans;
pub mod calls;
pub mod claims;
pub mod events;
pub mod ledger;
pub mod media;
pub mod members;
pub mod memberships;
pub mod methods;
pub mod notifications;
pub mod relays;
pub mod roles;
pub mod rooms;
pub mod sessions;
pub mod tenants;
pub mod usage;
