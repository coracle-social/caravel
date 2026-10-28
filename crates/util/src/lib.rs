//! The workspace's foundations: the clock, the change bus, the serde helpers, hex
//! and hashing, and the Postgres plumbing, plus clients for the systems outside
//! caravel's database. Nothing here knows what a tenant or a relay is.

// The foundations every layer builds on.
pub mod auth;
pub mod blank;
pub mod bus;
pub mod clock;
pub mod config;
pub mod db;
pub mod encoding;
pub mod error;
pub mod flag;
pub mod urls;

// Clients for the systems outside caravel's database. Each speaks one protocol and
// hands back plain values for the core to interpret.
pub mod bitcoin;
pub mod blobs;
pub mod dns;
pub mod livekit;
pub mod nostr;
pub mod outbound;
pub mod robot;
pub mod stripe;
pub mod wallet;
