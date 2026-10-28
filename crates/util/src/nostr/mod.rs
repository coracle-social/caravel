//! Nostr: the workspace's keys, a client for other people's relays, and the event
//! kinds that carry a meaning here.

pub mod client;
pub mod keys;
pub mod kinds;

pub use client::Client;
pub use keys::{Key, generate_keypair};
