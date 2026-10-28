//! The clients the relay gives up on: one that stops reading would otherwise hold
//! its connection open forever, stalling the relay's writes to it.

mod common;

use std::time::Duration;

use caravel_core::{ingest, testing};
use caravel_relay::{RelaysConfig, Transport};
use coracle_lib::keys::SecretKey;
use serde_json::json;

use common::Server;

async fn server(transport: Transport) -> Server {
    let core = testing::core(&SecretKey::generate().public_key().to_hex()).await;
    let config = RelaysConfig {
        transport,
        ..Default::default()
    };
    Server::start_configured(core, config).await
}

#[tokio::test]
async fn a_client_that_leaves_pings_unanswered_is_dropped() {
    let server = server(Transport {
        ping_interval: Duration::from_millis(50),
        pong_wait: Duration::from_millis(300),
        ..Default::default()
    })
    .await;
    let (relay, owner) = server.relay().await;
    let host = relay.canonical_host();

    // A client answers pings as it reads, so one that keeps reading stays.
    let mut reader = server.connect(&host).await;
    reader.listen(Duration::from_secs(1)).await;
    reader.auth(&owner, server.now()).await;

    let mut idle = server.connect(&host).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(idle.is_dropped().await);
}

#[tokio::test]
async fn a_client_that_stops_reading_is_dropped() {
    let server = server(Transport {
        write_wait: Duration::from_millis(300),
        ..Default::default()
    })
    .await;
    let (relay, owner) = server.relay().await;
    let key = server.core.relay_key(&relay).unwrap();

    // Far more than the socket buffers between the relay and the client hold.
    let content = "x".repeat(1024 * 1024);
    for age in 0..40 {
        let event = testing::sign_event(&owner, 1, &content, &[], server.now() - age);
        ingest::store(&server.core, &relay, &key, false, &event)
            .await
            .unwrap();
    }

    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&owner, server.now()).await;
    client
        .send(json!(["REQ", "everything", {"kinds": [1]}]))
        .await;

    // Without reading, the client fills the socket and stalls the relay's writes.
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(client.is_dropped().await);
}
