//! A core for tests: this test binary's database, a pinned clock, and adapters
//! pointed at closed ports so nothing leaves the machine.

use std::sync::Arc;

use caravel_adapters::blobs::Blobs;
use caravel_adapters::livekit::Livekit;
use caravel_adapters::outbound::Outbound;
use caravel_adapters::{robot::Robot, robot::RobotProfile, stripe::Stripe, wallet::Wallet};

use crate::Core;
use crate::clock::Clock;
use crate::config::Config;
use crate::crypto::{PlatformKey, generate_keypair};

/// The relay domain test relays are created under.
pub const RELAY_DOMAIN: &str = "relay.test";

/// The API base url test requests sign for.
pub const API_URL: &str = "http://api.test";

const DEAD_RELAY: &str = "ws://127.0.0.1:1";

/// The LiveKit server test cores call, which isn't there.
pub const LIVEKIT_URL: &str = "ws://127.0.0.1:1";

/// A core on this test binary's database, administered by `admin_pubkey`.
/// The platform key every test core shares, so any test's relays decrypt in any
/// other test's core on the same database.
const PLATFORM_SECRET: &str = "7f1d9a1c3c3f0e2b5a8d4c6e9f0a1b2c3d4e5f60718293a4b5c6d7e8f9011223";

pub async fn core(admin_pubkey: &str) -> Core {
    let robot_secret = PLATFORM_SECRET.to_string();
    let (wallet_pubkey, wallet_secret) = generate_keypair();
    let platform_key = PlatformKey::from_hex(&robot_secret).expect("invalid generated key");

    let config = Config {
        api_url: API_URL.to_string(),
        admin_pubkeys: vec![admin_pubkey.to_string()],
        relay_domains: vec![RELAY_DOMAIN.to_string()],
        member_fee_percent: 10.0,
        flotilla_url: "https://app.flotilla.test".to_string(),
        platform_key,
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
        blobs: Blobs::memory(),
        livekit: Some(
            Livekit::new(LIVEKIT_URL, "test-key", "test-secret").expect("invalid livekit"),
        ),
        outbound: Outbound::new(std::time::Duration::from_secs(5), true)
            .expect("invalid outbound client"),
    }
}

/// A signed event.
pub fn sign_event(
    secret: &coracle_lib::keys::SecretKey,
    kind: u16,
    content: &str,
    tags: &[&[&str]],
    created_at: i64,
) -> coracle_lib::events::Event {
    let tags: coracle_lib::tags::Tags = tags
        .iter()
        .map(|tag| coracle_lib::tags::Tag(tag.iter().map(|value| value.to_string()).collect()))
        .collect();
    let hashed = coracle_lib::events::EventContent::new()
        .with_content(content)
        .with_tags(tags)
        .with_kind(kind)
        .with_created_at(created_at)
        .with_pubkey(secret.public_key())
        .with_id();
    let sig = secret.sign(hashed.id.as_bytes());
    hashed.with_sig(sig)
}

/// A tenant and a relay of theirs on the free plan, without reaching Stripe.
/// Returns the relay and the tenant's secret key.
pub async fn create_relay(core: &Core) -> (crate::relays::Relay, coracle_lib::keys::SecretKey) {
    let secret = coracle_lib::keys::SecretKey::generate();
    let pubkey = secret.public_key().to_hex();

    crate::tenants::insert_tenant(
        core.db.pool(),
        &crate::tenants::Tenant {
            pubkey: pubkey.clone(),
            return_url: "http://localhost/account".to_string(),
            created_at: core.clock.now(),
            stripe_customer_id: format!("cus_{}", &pubkey[..12]),
            ..Default::default()
        },
    )
    .await
    .expect("failed to insert tenant");

    let relay = crate::hosting::create_relay(
        core,
        &pubkey,
        crate::hosting::RelaySettings {
            subdomain: Some(format!(
                "r{}",
                &uuid::Uuid::new_v4().simple().to_string()[..12]
            )),
            domain: Some(RELAY_DOMAIN.to_string()),
            plan_id: Some("free".to_string()),
            ..Default::default()
        },
    )
    .await
    .expect("failed to create relay");

    (relay, secret)
}
