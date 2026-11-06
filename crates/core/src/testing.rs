//! A core for tests: this test binary's database, a pinned clock, and adapters
//! pointed at closed ports so nothing leaves the machine.

use std::sync::Arc;

use caravel_util::blobs::Blobs;
use caravel_util::livekit::Livekit;
use caravel_util::outbound::Outbound;
use caravel_util::{robot::Robot, robot::RobotProfile, stripe::Stripe, wallet::Wallet};

use crate::Core;
use caravel_util::clock::Clock;
use caravel_util::config::Config;
use caravel_util::nostr::{Key, generate_keypair};

/// The relay domain test relays are created under.
pub const RELAY_DOMAIN: &str = "relay.test";

/// The platform's host in tests, under the relay domain as a deployment may put it.
pub const PLATFORM_HOST: &str = "app.relay.test";

/// The API base url test requests sign for.
pub const API_URL: &str = "http://app.relay.test/api";

/// The url the API answered at before sessions.
pub const LEGACY_API_URL: &str = "http://api.relay.test";

const DEAD_RELAY: &str = "ws://127.0.0.1:1";

/// The LiveKit server test cores call, which isn't there.
pub const LIVEKIT_URL: &str = "ws://127.0.0.1:1";

/// The platform key every test core shares, so any test's relays decrypt in any
/// other test's core on the same database.
const PLATFORM_SECRET: &str = "7f1d9a1c3c3f0e2b5a8d4c6e9f0a1b2c3d4e5f60718293a4b5c6d7e8f9011223";

/// A core on this test binary's database, administered by `admin_pubkey`.
pub async fn core(admin_pubkey: &str) -> Core {
    let robot_secret = PLATFORM_SECRET.to_string();
    let (wallet_pubkey, wallet_secret) = generate_keypair();
    let platform_key = Key::from_hex(&robot_secret).expect("invalid platform key");

    let config = Config {
        api_url: API_URL.to_string(),
        tenant_url: "http://app.relay.test".to_string(),
        admin_url: "http://app.relay.test/admin".to_string(),
        legacy_api_url: Some(LEGACY_API_URL.to_string()),
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
        db: caravel_util::db::testing::connect().await,
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
        outbound: Outbound::allowing_private(std::time::Duration::from_secs(5))
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

/// An `Authorization` header carrying a signed event, as NIP-98 and Blossom take.
pub fn authorization(event: &coracle_lib::events::Event) -> String {
    use base64::Engine;
    format!(
        "Nostr {}",
        base64::engine::general_purpose::STANDARD
            .encode(serde_json::to_string(event).expect("events serialize"))
    )
}

/// A NIP-98 `Authorization` header signed by `secret` for `url` and `method`.
pub fn http_auth(
    secret: &coracle_lib::keys::SecretKey,
    url: &str,
    method: &str,
    created_at: i64,
) -> String {
    let event = sign_event(
        secret,
        caravel_util::nostr::kinds::HTTP_AUTH,
        "",
        &[&["u", url], &["method", method]],
        created_at,
    );
    authorization(&event)
}

/// Make a pubkey a tenant, without reaching Stripe.
pub async fn insert_tenant(core: &Core, pubkey: &str) {
    caravel_billing::tenants::commands::insert_tenant(
        core.db.pool(),
        &caravel_billing::tenants::Tenant {
            pubkey: pubkey.to_string(),
            return_url: "http://localhost/account".to_string(),
            created_at: core.clock.now(),
            stripe_customer_id: format!("cus_{}", &pubkey[..12]),
            ..Default::default()
        },
    )
    .await
    .expect("failed to insert tenant");
}

/// A subdomain no other test's relay has.
pub fn subdomain() -> String {
    format!("r{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
}

/// A tenant and a relay of theirs on the free plan, without reaching Stripe.
/// Returns the relay and the tenant's secret key.
pub async fn create_relay(
    core: &Core,
) -> (caravel_billing::relays::Relay, coracle_lib::keys::SecretKey) {
    let secret = coracle_lib::keys::SecretKey::generate();
    let pubkey = secret.public_key().to_hex();
    insert_tenant(core, &pubkey).await;

    let relay = crate::hosting::create_relay(
        core,
        &pubkey,
        caravel_billing::relays::RelaySettings {
            subdomain: Some(subdomain()),
            domain: Some(RELAY_DOMAIN.to_string()),
            plan_id: Some("free".to_string()),
            ..Default::default()
        },
    )
    .await
    .expect("failed to create relay");

    (relay, secret)
}
