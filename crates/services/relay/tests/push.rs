//! NIP-9a push, ported from zooid's push tests: which subscriptions a relay takes,
//! and the callbacks it makes for the events they match.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use caravel_core::testing;
use caravel_core::testing::sign_event;
use caravel_domain::relays::Relay;
use caravel_util::nostr::kinds;
use caravel_util::outbound::Outbound;
use common::*;
use coracle_lib::events::Event;
use coracle_lib::keys::SecretKey;
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// A callback server answering with `status`, passing on what it receives.
async fn callback(status: StatusCode) -> (String, mpsc::UnboundedReceiver<Value>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let state = Arc::new(Mutex::new(sender));
    let router = Router::new()
        .route(
            "/push",
            post(
                move |State(sender): State<Arc<Mutex<mpsc::UnboundedSender<Value>>>>,
                      Json(body): Json<Value>| async move {
                    sender.lock().unwrap().send(body).ok();
                    status
                },
            ),
        )
        .with_state(state);
    let addr = serve_local(router).await;
    (format!("http://{addr}/push"), receiver)
}

fn subscription(
    server: &Server,
    relay: &Relay,
    signer: &SecretKey,
    d: &str,
    callback: &str,
    filter: Value,
) -> Event {
    let relay_url = format!("wss://{}/", relay.canonical_host());
    let filter = filter.to_string();
    sign_event(
        signer,
        30390,
        "",
        &[
            &["d", d],
            &["relay", &relay_url],
            &["filter", &filter],
            &["callback", callback],
            &["include_event"],
        ],
        server.now(),
    )
}

async fn member(server: &Server, relay: &Relay, owner: &SecretKey) -> SecretKey {
    let member = SecretKey::generate();
    server
        .manage(
            &relay.canonical_host(),
            owner,
            "allowpubkey",
            json!([member.public_key().to_hex()]),
        )
        .await;
    member
}

async fn next(receiver: &mut mpsc::UnboundedReceiver<Value>) -> Option<Value> {
    tokio::time::timeout(Duration::from_millis(500), receiver.recv())
        .await
        .ok()
        .flatten()
}

// Ported from zooid: a callback naming a local address is refused at once.
#[tokio::test]
async fn a_local_callback_is_refused() {
    let mut core = testing::core(&SecretKey::generate().public_key().to_hex()).await;
    core.outbound = Outbound::new(Duration::from_secs(1)).unwrap();
    let server = Server::start_with(core).await;
    let (relay, owner) = server.relay().await;
    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&owner, server.now()).await;

    let local = subscription(
        &server,
        &relay,
        &owner,
        "a",
        "http://127.0.0.1/push",
        json!({"kinds": [1]}),
    );
    let (accepted, message) = client.publish(&local).await;
    assert!(!accepted);
    assert_eq!(
        message,
        "invalid: callback must not point to a local or internal address"
    );

    let public = subscription(
        &server,
        &relay,
        &owner,
        "b",
        "https://push.example.com/push",
        json!({"kinds": [1]}),
    );
    assert!(client.publish(&public).await.0);
}

// Ported from zooid: the relay tag names this relay however it is spelled.
#[tokio::test]
async fn the_relay_tag_must_name_this_relay() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&owner, server.now()).await;

    for (relay_url, accepted) in [
        (format!("wss://{}", relay.canonical_host()), true),
        (
            format!("WSS://{}/", relay.canonical_host().to_uppercase()),
            true,
        ),
        ("wss://elsewhere.test/".to_string(), false),
    ] {
        let event = sign_event(
            &owner,
            30390,
            "",
            &[
                &["d", &relay_url],
                &["relay", &relay_url],
                &["filter", "{}"],
                &["callback", "https://push.example.com/"],
            ],
            server.now(),
        );
        assert_eq!(client.publish(&event).await.0, accepted, "{relay_url}");
    }
}

#[tokio::test]
async fn a_pubkey_holds_at_most_ten_subscriptions() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&owner, server.now()).await;

    let callback = "https://push.example.com/";
    for i in 0..10 {
        let event = subscription(&server, &relay, &owner, &i.to_string(), callback, json!({}));
        assert!(client.publish(&event).await.0, "subscription {i}");
    }

    let eleventh = subscription(&server, &relay, &owner, "10", callback, json!({}));
    let (accepted, message) = client.publish(&eleventh).await;
    assert!(!accepted);
    assert_eq!(message, "invalid: too many subscriptions registered");

    // Replacing one isn't another.
    server.core.clock.pin(server.now() + 1);
    let replacement = subscription(
        &server,
        &relay,
        &owner,
        "3",
        callback,
        json!({"kinds": [1]}),
    );
    assert!(client.publish(&replacement).await.0);
}

#[tokio::test]
async fn a_matching_event_is_pushed_to_other_members() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let author = member(&server, &relay, &owner).await;
    let (url, mut pushed) = callback(StatusCode::OK).await;

    let mut subscriber = server.connect(&relay.canonical_host()).await;
    subscriber.auth(&owner, server.now()).await;
    let filter = json!({"kinds": [1]});
    assert!(
        subscriber
            .publish(&subscription(&server, &relay, &owner, "a", &url, filter))
            .await
            .0
    );

    let mut writer = server.connect(&relay.canonical_host()).await;
    writer.auth(&author, server.now()).await;
    let note = sign_event(&author, 1, "hello", &[], server.now());
    assert!(writer.publish(&note).await.0);

    let payload = next(&mut pushed).await.expect("no push");
    assert_eq!(payload["id"], note.id.to_hex());
    assert_eq!(
        payload["relay"],
        format!("wss://{}/", relay.canonical_host())
    );
    assert_eq!(payload["event"]["content"], "hello");

    // Neither another kind nor the subscriber's own events are pushed.
    assert!(
        writer
            .publish(&sign_event(&author, 7, "+", &[], server.now()))
            .await
            .0
    );
    assert!(
        subscriber
            .publish(&sign_event(&owner, 1, "mine", &[], server.now()))
            .await
            .0
    );
    assert!(next(&mut pushed).await.is_none());
}

// The relay's own events, whether a NIP-86 call or a client's event led to them.
#[tokio::test]
async fn the_relays_own_events_are_pushed_like_a_members() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let (url, mut pushed) = callback(StatusCode::OK).await;

    let mut subscriber = server.connect(&relay.canonical_host()).await;
    subscriber.auth(&owner, server.now()).await;
    let filter = json!({"kinds": [kinds::RELAY_ADD_MEMBER, kinds::RELAY_REMOVE_MEMBER]});
    assert!(
        subscriber
            .publish(&subscription(&server, &relay, &owner, "a", &url, filter))
            .await
            .0
    );

    let joined = member(&server, &relay, &owner).await;
    let payload = next(&mut pushed).await.expect("no push for the addition");
    assert_eq!(payload["event"]["kind"], kinds::RELAY_ADD_MEMBER);
    assert_eq!(payload["event"]["pubkey"], relay.pubkey);

    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&joined, server.now()).await;
    let leave = sign_event(&joined, kinds::RELAY_LEAVE, "", &[], server.now());
    assert!(client.publish(&leave).await.0);
    let payload = next(&mut pushed).await.expect("no push for the removal");
    assert_eq!(payload["event"]["kind"], kinds::RELAY_REMOVE_MEMBER);
    assert_eq!(payload["event"]["pubkey"], relay.pubkey);
}

// zooid took subscriptions from anyone and pushed to every one; a relay's events
// only reach those who may read it.
#[tokio::test]
async fn an_outsider_is_not_pushed_a_closed_relays_events() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let outsider = SecretKey::generate();
    let (url, mut pushed) = callback(StatusCode::OK).await;
    let filter = json!({"kinds": [1]});

    let mut stranger = server.connect(&relay.canonical_host()).await;
    stranger.auth(&outsider, server.now()).await;
    let event = subscription(&server, &relay, &outsider, "a", &url, filter.clone());
    assert_eq!(
        stranger.publish(&event).await,
        (
            false,
            "restricted: you are not a member of this relay".to_string()
        )
    );

    // A member's subscription outlives their membership, but is pushed nothing after.
    let former = member(&server, &relay, &owner).await;
    let mut subscriber = server.connect(&relay.canonical_host()).await;
    subscriber.auth(&former, server.now()).await;
    let event = subscription(&server, &relay, &former, "a", &url, filter);
    assert!(subscriber.publish(&event).await.0);

    let mut writer = server.connect(&relay.canonical_host()).await;
    writer.auth(&owner, server.now()).await;
    let note = |content: &str| sign_event(&owner, 1, content, &[], server.now());
    assert!(writer.publish(&note("while a member")).await.0);
    assert!(next(&mut pushed).await.is_some());

    server
        .manage(
            &relay.canonical_host(),
            &owner,
            "unallowpubkey",
            json!([former.public_key().to_hex()]),
        )
        .await;
    assert!(writer.publish(&note("members only")).await.0);
    assert!(next(&mut pushed).await.is_none());
}

#[tokio::test]
async fn a_callback_answering_404_drops_its_subscription() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let author = member(&server, &relay, &owner).await;
    let (url, mut pushed) = callback(StatusCode::NOT_FOUND).await;

    let mut subscriber = server.connect(&relay.canonical_host()).await;
    subscriber.auth(&owner, server.now()).await;
    assert!(
        subscriber
            .publish(&subscription(
                &server,
                &relay,
                &owner,
                "a",
                &url,
                json!({"kinds": [1]})
            ))
            .await
            .0
    );

    let mut writer = server.connect(&relay.canonical_host()).await;
    writer.auth(&author, server.now()).await;
    assert!(
        writer
            .publish(&sign_event(&author, 1, "first", &[], server.now()))
            .await
            .0
    );
    assert!(next(&mut pushed).await.is_some());

    // The relay forgets the subscription, so the next event goes nowhere.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        writer
            .publish(&sign_event(&author, 1, "second", &[], server.now()))
            .await
            .0
    );
    assert!(next(&mut pushed).await.is_none());

    let (stored, _) = subscriber.req("subs", json!({"kinds": [30390]})).await;
    assert!(stored.is_empty());
}
