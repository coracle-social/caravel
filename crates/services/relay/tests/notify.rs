//! NOTIFY, ported from zooid's notify tests: a pubkey's message reaches every
//! connection authenticated as it, when it authenticates and while it stays.

mod common;

use caravel_core::notifications;
use common::*;
use coracle_lib::keys::SecretKey;
use serde_json::json;

#[tokio::test]
async fn a_waiting_message_arrives_on_authentication() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    notifications::notify(
        &server.core,
        &relay.id,
        &owner.public_key().to_hex(),
        "welcome back",
        None,
    )
    .await
    .unwrap();

    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&owner, server.now()).await;
    assert_eq!(
        client.recv().await.unwrap(),
        json!(["NOTIFY", "welcome back"])
    );
}

#[tokio::test]
async fn a_new_message_reaches_its_pubkey_alone() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let other = SecretKey::generate();

    let mut owners = server.connect(&relay.canonical_host()).await;
    owners.auth(&owner, server.now()).await;
    let mut others = server.connect(&relay.canonical_host()).await;
    others.auth(&other, server.now()).await;
    let mut anonymous = server.connect(&relay.canonical_host()).await;

    notifications::notify(
        &server.core,
        &relay.id,
        &owner.public_key().to_hex(),
        "renew soon",
        None,
    )
    .await
    .unwrap();

    assert_eq!(
        owners.recv().await.unwrap(),
        json!(["NOTIFY", "renew soon"])
    );
    assert!(others.is_quiet().await);
    assert!(anonymous.is_quiet().await);
}

#[tokio::test]
async fn an_expired_message_is_not_sent() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let expires_at = server.now() - 1;
    notifications::notify(
        &server.core,
        &relay.id,
        &owner.public_key().to_hex(),
        "too late",
        Some(expires_at),
    )
    .await
    .unwrap();

    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&owner, server.now()).await;
    assert!(client.is_quiet().await);
}
