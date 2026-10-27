//! The adapters' Nostr client and the robot, against a relay served here: they
//! answer its challenge, publish, read, and wait for an answer to a request.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use caravel_adapters::nostr;
use caravel_adapters::robot::{Robot, RobotProfile};
use caravel_core::commands;
use caravel_core::models::relays::Relay;
use caravel_core::testing::sign_event;
use common::*;
use coracle_kinds::gift_wrap::{GiftWrapReader, SealReader};
use coracle_lib::filters::Filter;
use coracle_lib::keys::SecretKey;
use coracle_lib::readers::Reader;
use serde_json::json;

const TIMEOUT: Duration = Duration::from_secs(5);

/// A readable relay that also answers on `host`, and its url there.
async fn relay_at(server: &Server, host: &str) -> (Relay, SecretKey, String) {
    let (relay, owner) = server.relay().await;
    let relay = server
        .update(&relay, json!({"policy_public_read": 1}))
        .await;
    commands::relays::add_host_redirect(server.core.db.pool(), host, &relay.id, server.now())
        .await
        .unwrap();
    server.relays.reload(&relay.id).await.unwrap();
    let url = format!("ws://{host}:{}/", server.addr.port());
    (relay, owner, url)
}

#[tokio::test]
async fn the_robot_sends_a_dm_its_recipient_can_open() {
    let server = Server::start().await;
    let (_, owner, url) = relay_at(&server, "localhost").await;
    let now = server.now();
    let relays = [url.clone()];

    // The recipient lists this relay as their outbox and their inbox.
    let member = nostr::Client::allowing_private(owner.clone());
    for (kind, tag) in [(10002, "r"), (10050, "relay")] {
        let list = sign_event(&owner, kind, "", &[&[tag, &url]], now);
        member.publish(&relays, &list, now, TIMEOUT).await.unwrap();
    }

    let robot_secret = SecretKey::generate();
    let profile = RobotProfile {
        indexer_relays: relays.to_vec(),
        ..Default::default()
    };
    let robot = Robot::allowing_private(&robot_secret.to_hex(), profile).unwrap();
    robot
        .send_dm(&owner.public_key().to_hex(), "your invoice is ready", now)
        .await
        .unwrap();

    let wraps = Filter {
        kinds: Some(BTreeSet::from([1059])),
        tags: BTreeMap::from([(
            "#p".to_string(),
            BTreeSet::from([owner.public_key().to_hex()]),
        )]),
        ..Default::default()
    };
    let received = member.fetch(&relays, &wraps, now, TIMEOUT).await.unwrap();
    assert_eq!(received.len(), 1);

    let wrap = &received[0];
    let seal = GiftWrapReader::read(wrap)
        .unwrap()
        .open_seal(&owner.nip44_decrypt(&wrap.pubkey, &wrap.content).unwrap())
        .unwrap();
    let rumor = SealReader::read(&seal)
        .unwrap()
        .open_rumor(&owner.nip44_decrypt(&seal.pubkey, &seal.content).unwrap())
        .unwrap();
    assert_eq!(rumor.pubkey, robot_secret.public_key());
    assert_eq!(rumor.content, "your invoice is ready");
}

#[tokio::test]
async fn a_request_waits_for_the_event_that_answers_it() {
    let server = Server::start().await;
    let (relay, owner, url) = relay_at(&server, "127.0.0.1").await;
    let now = server.now();

    // A responder listening for requests on the relay.
    let mut responder = server.connect(&relay.canonical_host()).await;
    responder.auth(&owner, now).await;
    responder.req("requests", json!({"kinds": [23194]})).await;

    let request = sign_event(&owner, 23194, "ping", &[], now);
    let reply = Filter {
        kinds: Some(BTreeSet::from([23195])),
        tags: BTreeMap::from([("#e".to_string(), BTreeSet::from([request.id.to_hex()]))]),
        ..Default::default()
    };
    let requester = nostr::Client::allowing_private(owner.clone());
    let relays = [url];
    let asking = requester.request(&relays, &request, &reply, now, TIMEOUT);
    let answering = async {
        loop {
            let message = responder.recv().await.unwrap();
            if message[0] == "EVENT" && message[1] == "requests" {
                let id = message[2]["id"].as_str().unwrap().to_string();
                let answer = sign_event(&owner, 23195, "pong", &[&["e", &id]], now);
                responder.publish(&answer).await;
                return;
            }
        }
    };

    let (answer, ()) = tokio::join!(asking, answering);
    assert_eq!(answer.unwrap().content, "pong");
}
