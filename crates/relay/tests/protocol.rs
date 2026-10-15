//! The relays over a real socket, each reached by its own host: the NIP-01
//! conversation, NIP-42 authentication, live delivery, and the walls between
//! relays.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use caravel_core::testing::sign_event;
use common::*;
use coracle_lib::events::EventId;
use coracle_lib::keys::SecretKey;
use coracle_lib::sync::{
    FrameBudget, Item, Message as SyncMessage, SyncSet, initiate, reconcile_initiator,
};
use serde_json::{Value, json};

#[tokio::test]
async fn the_information_document_describes_the_relay() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let route = server
        .relays
        .resolve(&relay.canonical_host())
        .expect("relay not served");

    let request = Request::builder()
        .uri("/")
        .header("Host", relay.canonical_host())
        .header("Accept", "application/nostr+json")
        .body(Body::empty())
        .unwrap();
    let response = server.relays.serve(route, request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["access-control-allow-origin"], "*");

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let document: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(document["pubkey"], owner.public_key().to_hex());
    assert_eq!(document["self"], relay.pubkey);
    assert_eq!(document["limitation"]["auth_required"], true);
    assert_eq!(document["limitation"]["created_at_upper_limit"], 300);
    assert!(
        document["supported_nips"]
            .as_array()
            .unwrap()
            .contains(&json!(42))
    );
}

#[tokio::test]
async fn reading_needs_authentication_and_offers_the_challenge_again() {
    let server = Server::start().await;
    let (relay, _) = server.relay().await;
    let mut client = server.connect(&relay.canonical_host()).await;

    client.send(json!(["REQ", "sub", {}])).await;
    let challenge = client.recv().await.unwrap();
    assert_eq!(challenge, json!(["AUTH", client.challenge]));
    let closed = client.recv().await.unwrap();
    assert_eq!(closed[0], "CLOSED");
    assert!(closed[2].as_str().unwrap().starts_with("auth-required:"));
}

#[tokio::test]
async fn the_owner_publishes_and_a_subscriber_sees_it_live_and_stored() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let host = relay.canonical_host();

    let mut reader = server.connect(&host).await;
    reader.auth(&owner, server.now()).await;
    let (stored, eose) = reader.req("live", json!({"kinds": [1]})).await;
    assert!(stored.is_empty());
    assert_eq!(eose[0], "EOSE");

    let mut writer = server.connect(&host).await;
    writer.auth(&owner, server.now()).await;
    let event = sign_event(&owner, 1, "hello", &[], server.now());
    assert_eq!(writer.publish(&event).await, (true, String::new()));

    let delivered = reader.recv().await.unwrap();
    assert_eq!(delivered[0], "EVENT");
    assert_eq!(delivered[1], "live");
    assert_eq!(delivered[2]["id"], event.id.to_hex());
    assert_eq!(delivered[2]["sig"], hex::encode(event.sig));

    let (stored, _) = writer
        .req("stored", json!({"ids": [event.id.to_hex()]}))
        .await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0]["content"], "hello");

    // Publishing it again is a duplicate, which is accepted and not redelivered.
    let (accepted, message) = writer.publish(&event).await;
    assert!(accepted);
    assert!(message.starts_with("duplicate:"));
    assert!(reader.is_quiet().await);
}

#[tokio::test]
async fn strangers_may_neither_read_nor_write_a_private_relay() {
    let server = Server::start().await;
    let (relay, _) = server.relay().await;
    let stranger = SecretKey::generate();
    let mut client = server.connect(&relay.canonical_host()).await;

    let event = sign_event(&stranger, 1, "hi", &[], server.now());
    let (accepted, message) = client.publish(&event).await;
    assert!(!accepted);
    assert!(message.starts_with("auth-required:"), "{message}");

    client.auth(&stranger, server.now()).await;
    let (accepted, message) = client.publish(&event).await;
    assert!(!accepted);
    assert_eq!(message, "restricted: you are not a member of this relay");

    let (_, closed) = client.req("sub", json!({})).await;
    assert_eq!(closed[0], "CLOSED");
    assert_eq!(closed[2], "restricted: you are not a member of this relay");
}

#[tokio::test]
async fn an_answer_for_another_relay_does_not_authenticate() {
    let server = Server::start().await;
    let (first, owner) = server.relay().await;
    let (second, _) = server.relay().await;
    let mut client = server.connect(&first.canonical_host()).await;

    let wrong = format!("wss://{}/", second.canonical_host());
    let answer = sign_event(
        &owner,
        22242,
        "",
        &[&["relay", &wrong], &["challenge", &client.challenge]],
        server.now(),
    );
    client.send(json!(["AUTH", answer])).await;
    let ok = client.recv().await.unwrap();
    assert_eq!(ok[2], false);
    assert!(ok[3].as_str().unwrap().starts_with("invalid:"));
}

#[tokio::test]
async fn relays_are_walled_off_from_each_other() {
    let server = Server::start().await;
    let (first, owner) = server.relay().await;
    let (second, second_owner) = server.relay().await;

    let mut writer = server.connect(&first.canonical_host()).await;
    writer.auth(&owner, server.now()).await;
    let mut other = server.connect(&second.canonical_host()).await;
    other.auth(&second_owner, server.now()).await;
    other.req("everything", json!({"kinds": [1]})).await;

    let event = sign_event(&owner, 1, "only here", &[], server.now());
    assert!(writer.publish(&event).await.0);

    assert!(
        other.is_quiet().await,
        "an event never crosses to another relay live"
    );
    let (stored, _) = other.req("again", json!({"kinds": [1]})).await;
    assert!(stored.is_empty(), "or stored");
}

#[tokio::test]
async fn a_public_relay_strips_signatures_for_everyone_but_its_managers() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    server
        .update(
            &relay,
            json!({"policy_public_read": 1, "policy_strip_signatures": 1}),
        )
        .await;
    let host = relay.canonical_host();

    let mut writer = server.connect(&host).await;
    writer.auth(&owner, server.now()).await;
    let event = sign_event(&owner, 1, "signed", &[], server.now());
    assert!(writer.publish(&event).await.0);

    let mut stranger = server.connect(&host).await;
    stranger.auth(&SecretKey::generate(), server.now()).await;
    let (stored, _) = stranger.req("sub", json!({"kinds": [1]})).await;
    assert_eq!(stored[0]["sig"], "0".repeat(128));

    let (stored, _) = writer.req("sub", json!({"kinds": [1]})).await;
    assert_eq!(stored[0]["sig"], hex::encode(event.sig));
}

#[tokio::test]
async fn an_anonymous_relay_serves_readers_who_never_authenticate() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    server
        .update(&relay, json!({"policy_anonymous_read": 1}))
        .await;
    let host = relay.canonical_host();

    let mut writer = server.connect(&host).await;
    writer.auth(&owner, server.now()).await;
    let event = sign_event(&owner, 1, "for anyone", &[], server.now());
    assert!(writer.publish(&event).await.0);

    let mut anonymous = server.connect(&host).await;
    let (stored, eose) = anonymous.req("sub", json!({"kinds": [1]})).await;
    assert_eq!(eose[0], "EOSE");
    assert_eq!(stored.len(), 1);
}

#[tokio::test]
async fn a_deletion_removes_the_event_from_later_reads() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&owner, server.now()).await;

    let event = sign_event(&owner, 1, "regret", &[], server.now());
    assert!(client.publish(&event).await.0);
    let deletion = sign_event(&owner, 5, "", &[&["e", &event.id.to_hex()]], server.now());
    assert!(client.publish(&deletion).await.0);

    let (stored, _) = client.req("sub", json!({"kinds": [1]})).await;
    assert!(stored.is_empty());
    let (accepted, message) = client.publish(&event).await;
    assert!(!accepted);
    assert_eq!(message, "blocked: this event was deleted");
}

#[tokio::test]
async fn negentropy_reconciles_a_client_against_what_it_may_read() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&owner, server.now()).await;

    let mut published = Vec::new();
    for i in 0..3 {
        let event = sign_event(&owner, 1, &i.to_string(), &[], server.now() - i);
        assert!(client.publish(&event).await.0);
        published.push(event.id);
    }

    let local = SyncSet::new();
    client
        .send(json!(["NEG-OPEN", "sync", {"kinds": [1]}, initiate(&local).to_hex()]))
        .await;
    let reply = client.recv().await.unwrap();
    assert_eq!(reply[0], "NEG-MSG");

    let incoming = SyncMessage::from_hex(reply[2].as_str().unwrap()).unwrap();
    let output = reconcile_initiator(&local, &incoming, FrameBudget::unlimited());
    let mut need: Vec<EventId> = output.need;
    need.sort();
    published.sort();
    assert_eq!(need, published);

    // A client holding everything already has nothing to fetch.
    let full = SyncSet::from_items(published.iter().map(|id| Item {
        timestamp: 0,
        id: *id,
    }));
    assert!(!full.is_empty());
    client.send(json!(["NEG-CLOSE", "sync"])).await;
    assert!(client.is_quiet().await);
}

#[tokio::test]
async fn a_deactivated_relay_closes_its_connections_and_stops_answering() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let host = relay.canonical_host();
    let mut client = server.connect(&host).await;
    client.auth(&owner, server.now()).await;

    caravel_core::hosting::deactivate_relay(&server.core, &relay)
        .await
        .unwrap();
    server.relays.reload(&relay.id).await.unwrap();

    assert!(client.recv().await.is_err(), "the connection is closed");
    assert!(server.relays.resolve(&host).is_none());
}

#[tokio::test]
async fn a_verified_custom_domain_and_the_canonical_host_both_serve_the_relay() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    server
        .update(&relay, json!({"custom_domain": "chat.example.com"}))
        .await;
    caravel_core::relays::set_custom_domain_verified(
        server.core.db.pool(),
        &relay.id,
        "chat.example.com",
        true,
    )
    .await
    .unwrap();
    server.relays.reload(&relay.id).await.unwrap();

    let custom = server
        .relays
        .resolve("chat.example.com")
        .expect("custom domain not served");
    assert_eq!(custom.primary_host, "chat.example.com");
    let canonical = server
        .relays
        .resolve(&relay.canonical_host())
        .expect("canonical host not served");
    assert_eq!(canonical.primary_host, "chat.example.com");

    // A page on the canonical host goes to the custom domain; the protocol doesn't.
    let request = Request::builder().uri("/").body(Body::empty()).unwrap();
    let response = server.relays.serve(canonical, request).await;
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);

    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&owner, server.now()).await;
    let mut custom_client = server.connect("chat.example.com").await;
    custom_client.auth(&owner, server.now()).await;
}

#[tokio::test]
async fn shutting_down_closes_every_connection_as_going_away() {
    let server = Server::start().await;
    let (relay, _) = server.relay().await;
    let mut client = server.connect(&relay.canonical_host()).await;

    server.shutdown.cancel();
    let error = client.recv().await.unwrap_err().to_string();
    assert!(error.contains("1001") || error.contains("Away"), "{error}");
}
