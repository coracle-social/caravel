//! The platform's domain logic and persistence, in layers: models, the pure rules
//! over them, the queries that read them and the commands that write them, and
//! the subsystems that run those against a [`Core`], which carries the database,
//! the clock, the bus and the adapters.

// The foundations every layer builds on; only subsystems use the bus and the clock.
pub mod blank;
pub mod bus;
pub mod clock;
pub mod config;
pub mod crypto;
pub mod error;
pub mod flag;

// The layers: models, then domain, queries and commands, each using only those
// before it.
pub mod commands;
pub mod domain;
pub mod models;
pub mod queries;

// The subsystems, which run the layers against a `Core`.
pub mod billing;
pub mod calls;
pub mod domains;
pub mod events;
pub mod hosting;
pub mod ingest;
pub mod management;
pub mod media;
pub mod memberships;
pub mod notifications;
pub mod payouts;
pub mod sessions;
pub mod transfer;
pub mod usage;

#[cfg(feature = "testing")]
pub mod testing;

use std::sync::Arc;

use caravel_adapters::blobs::Blobs;
use caravel_adapters::livekit::Livekit;
use caravel_adapters::outbound::Outbound;
use caravel_adapters::{robot::Robot, stripe::Stripe, wallet::Wallet};
use caravel_db::Db;

pub use error::{Error, Result};

/// The database, configuration and adapters every core operation runs against.
#[derive(Clone)]
pub struct Core {
    pub db: Db,
    pub config: Arc<config::Config>,
    pub clock: clock::Clock,
    pub bus: bus::Bus,
    pub stripe: Stripe,
    pub robot: Robot,
    /// The platform's wallet, which issues the Lightning invoices tenants pay.
    pub wallet: Wallet,
    /// Where relays' media is stored.
    pub blobs: Blobs,
    /// The LiveKit server, when calls are set up.
    pub livekit: Option<Livekit>,
    /// Requests to urls other people chose: push callbacks and mirrored media.
    pub outbound: Outbound,
}

impl Core {
    /// The same core on another pool, so background work can't starve requests.
    pub fn with_db(&self, db: Db) -> Self {
        Self { db, ..self.clone() }
    }

    /// A relay's own key, decrypted with the platform key.
    pub fn relay_key(&self, relay: &models::relays::Relay) -> Result<crypto::RelayKey> {
        Ok(crypto::RelayKey::decrypt(
            &self.config.platform_key,
            &relay.secret,
        )?)
    }
}
