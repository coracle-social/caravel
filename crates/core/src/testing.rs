//! A core for tests: this test binary's database, a pinned clock, and adapters
//! pointed at closed ports so nothing leaves the machine.

use std::sync::Arc;

use caravel_adapters::{
    robot::Robot, robot::RobotProfile, stripe::Stripe, wallet::Wallet, zooid::Zooid,
};

use crate::Core;
use crate::clock::Clock;
use crate::config::Config;
use crate::crypto::{PlatformKey, generate_keypair};

/// The relay domain test relays are created under.
pub const RELAY_DOMAIN: &str = "relay.test";

/// The API base url test requests sign for.
pub const API_URL: &str = "http://api.test";

const DEAD_RELAY: &str = "ws://127.0.0.1:1";

/// A core on this test binary's database, administered by `admin_pubkey`.
pub async fn core(admin_pubkey: &str) -> Core {
    let (_, robot_secret) = generate_keypair();
    let (wallet_pubkey, wallet_secret) = generate_keypair();
    let platform_key = PlatformKey::from_hex(&robot_secret).expect("invalid generated key");

    let config = Config {
        api_url: API_URL.to_string(),
        admin_pubkeys: vec![admin_pubkey.to_string()],
        relay_domains: vec![RELAY_DOMAIN.to_string()],
        platform_key,
        blossom_s3: Default::default(),
        livekit: Default::default(),
        zooid_api_host: "zooid.test".to_string(),
    };

    let profile = RobotProfile {
        name: "caravel-test".to_string(),
        outbox_relays: vec![DEAD_RELAY.to_string()],
        indexer_relays: vec![DEAD_RELAY.to_string()],
        messaging_relays: vec![DEAD_RELAY.to_string()],
        ..Default::default()
    };

    Core {
        db: caravel_db::testing::connect().await,
        config: Arc::new(config),
        clock: Clock::default(),
        bus: Default::default(),
        stripe: Stripe::new("sk_test_none"),
        robot: Robot::new(&robot_secret, profile).expect("invalid robot key"),
        wallet: Wallet::from_url(&format!(
            "nostr+walletconnect://{wallet_pubkey}?relay={DEAD_RELAY}&secret={wallet_secret}"
        ))
        .expect("invalid wallet url"),
        zooid: Zooid::new("http://127.0.0.1:1", &robot_secret).expect("invalid zooid key"),
    }
}
