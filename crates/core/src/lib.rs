//! The platform's domain logic and persistence, partitioned by concern. Every
//! operation takes a [`Core`], which carries the database and the adapters.

pub mod access;
pub mod auth;
pub mod billing;
pub mod bus;
pub mod calls;
pub mod clock;
pub mod config;
pub mod crypto;
pub mod domains;
pub mod error;
pub mod events;
pub mod flag;
pub mod hosting;
pub mod management;
pub mod media;
pub mod memberships;
pub mod moderation;
pub mod notifications;
pub mod payouts;
pub mod plans;
pub mod relay_policy;
pub mod relays;
pub mod rooms;
pub mod tenants;
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
}
