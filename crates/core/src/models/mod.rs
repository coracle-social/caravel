//! The platform's data: the types every layer above reads and writes, with the
//! methods that read only their own fields. Nothing here touches the database, the
//! clock or the outside world.

pub mod activity;
pub mod auth;
pub mod bans;
pub mod billing;
pub mod events;
pub mod kinds;
pub mod ledger;
pub mod management;
pub mod media;
pub mod members;
pub mod memberships;
pub mod methods;
pub mod moderation;
pub mod notifications;
pub mod payouts;
pub mod plans;
pub mod policy;
pub mod relays;
pub mod roles;
pub mod rooms;
pub mod sessions;
pub mod tenants;
pub mod transfer;
pub mod usage;
