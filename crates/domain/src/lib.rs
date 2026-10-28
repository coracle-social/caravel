//! The platform's shared data: the types every layer reads and writes, and the pure
//! rules over them, as one pod per concern.

pub mod activity;
pub mod auth;
pub mod bans;
pub mod billing;
pub mod calls;
pub mod config;
pub mod error;
pub mod events;
pub mod management;
pub mod media;
pub mod memberships;
pub mod moderation;
pub mod notifications;
pub mod policy;
pub mod relays;
pub mod roles;
pub mod rooms;
pub mod sessions;
pub mod tenants;
pub mod transfer;
pub mod usage;

pub use error::{Error, Result};
