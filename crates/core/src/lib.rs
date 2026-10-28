//! The engine: the subsystems that run the domain's rules against a [`Core`],
//! which carries the database, the configuration, the clock, the bus and the
//! adapters. Each subsystem reads the clock, opens transactions, calls adapters
//! and publishes changes on the bus.

// The subsystems, which run the database layer against a `Core`.
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

use caravel_util::config::Config;
use caravel_util::blobs::Blobs;
use caravel_util::bus::Bus;
use caravel_util::clock::Clock;
use caravel_util::db::Db;
use caravel_util::livekit::Livekit;
use caravel_util::nostr::Key;
use caravel_util::outbound::Outbound;
use caravel_util::{robot::Robot, stripe::Stripe, wallet::Wallet};

pub use caravel_util::error::{Error, Result};

/// The database, configuration and adapters every operation runs against.
#[derive(Clone)]
pub struct Core {
    pub db: Db,
    pub config: Arc<Config>,
    pub clock: Clock,
    pub bus: Bus,
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
    pub fn relay_key(&self, relay: &caravel_domain::relays::Relay) -> Result<Key> {
        Ok(Key::from_hex(
            &self.config.platform_key.decrypt(&relay.secret)?,
        )?)
    }
}
