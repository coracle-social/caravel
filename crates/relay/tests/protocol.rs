//! The relays over a real socket, each reached by its own host: the NIP-01
//! conversation, NIP-42 authentication, live delivery, and the walls between
//! relays.

mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use caravel_core::testing::{self, sign_event};
use caravel_relay::{RelayLimits, Relays, RelaysConfig};
use caravel_util::nostr::kinds;
use common::*;
use coracle_lib::events::EventId;
use coracle_lib::keys::SecretKey;
use coracle_lib::sync::{
    FrameBudget, Item, Message as SyncMessage, SyncSet, initiate, reconcile_initiator,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

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
    assert_eq!(document["limitation"]["restricted_writes"], true);
    let nips = document["supported_nips"].as_array().unwrap().clone();
    for nip in [
        json!(1),
        json!(29),
        json!(42),
        json!(43),
        json!(45),
        json!(77),
        json!(86),
        json!("9a"),
    ] {
        assert!(nips.contains(&nip), "{nip} missing from {nips:?}");
    }

    // The document follows the relay's policy and the features it has on.
    let relay = server
        .update(
            &relay,
            json!({
                "policy_public_read": 1,
                "policy_anonymous_read": 1,
                "policy_public_write": 1,
                "groups_enabled": 0,
                "management_enabled": 0,
                "push_enabled": 0,
            }),
        )
        .await;
    let document = server
        .http(
            Request::builder()
                .uri("/")
                .header("Host", relay.canonical_host())
                .header("Accept", "application/nostr+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .json();
    assert_eq!(document["limitation"]["auth_required"], false);
    assert_eq!(document["limitation"]["restricted_writes"], false);
    let nips = document["supported_nips"].as_array().unwrap();
    for nip in [json!(29), json!(86), json!("9a")] {
        assert!(!nips.contains(&nip), "{nip} still in {nips:?}");
    }
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

    // A write that needs authentication offers the challenge again first.
    let event = sign_event(&stranger, 1, "hi", &[], server.now());
    client.send(json!(["EVENT", event])).await;
    assert_eq!(
        client.recv().await.unwrap(),
        json!(["AUTH", client.challenge])
    );
    let ok = client.recv().await.unwrap();
    assert_eq!(ok[2], false);
    assert!(
        ok[3].as_str().unwrap().starts_with("auth-required:"),
        "{ok}"
    );

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
        kinds::CLIENT_AUTH,
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
    assert!(stored[0].get("sig").is_none(), "{}", stored[0]);
    assert_eq!(stored[0]["id"], event.id.to_hex());

    let (stored, _) = writer.req("sub", json!({"kinds": [1]})).await;
    assert_eq!(stored[0]["sig"], hex::encode(event.sig));

    // Live delivery strips them the same way.
    let later = sign_event(&owner, 1, "later", &[], server.now());
    assert!(writer.publish(&later).await.0);
    let live = stranger.recv_skipping_auth().await.unwrap();
    assert_eq!(live[0], "EVENT");
    assert_eq!(live[2]["id"], later.id.to_hex());
    assert!(live[2].get("sig").is_none(), "{live}");
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

    let later = sign_event(&owner, 1, "and later", &[], server.now());
    assert!(writer.publish(&later).await.0);
    let delivered = anonymous.recv().await.unwrap();
    assert_eq!(delivered[1], "sub");
    assert_eq!(delivered[2]["id"], later.id.to_hex());
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
        published.push(event);
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
    let mut ids: Vec<EventId> = published.iter().map(|event| event.id).collect();
    ids.sort();
    assert_eq!(need, ids);
    client.send(json!(["NEG-CLOSE", "sync"])).await;
    assert!(client.is_quiet().await);

    // A client holding everything already has nothing to fetch or send.
    let full = SyncSet::from_items(published.iter().map(|event| Item {
        timestamp: event.created_at,
        id: event.id,
    }));
    client
        .send(json!(["NEG-OPEN", "full", {"kinds": [1]}, initiate(&full).to_hex()]))
        .await;
    let reply = client.recv().await.unwrap();
    assert_eq!(reply[0], "NEG-MSG");
    let incoming = SyncMessage::from_hex(reply[2].as_str().unwrap()).unwrap();
    let output = reconcile_initiator(&full, &incoming, FrameBudget::unlimited());
    assert!(output.need.is_empty(), "{:?}", output.need);
    assert!(output.have.is_empty(), "{:?}", output.have);
    assert!(output.next.is_none());

    // Reconciling needs authentication like reading does.
    let mut anonymous = server.connect(&relay.canonical_host()).await;
    anonymous
        .send(json!(["NEG-OPEN", "sync", {"kinds": [1]}, initiate(&local).to_hex()]))
        .await;
    assert_eq!(
        anonymous.recv().await.unwrap(),
        json!(["AUTH", anonymous.challenge])
    );
    let refused = anonymous.recv().await.unwrap();
    assert_eq!(refused[0], "NEG-ERR");
    assert_eq!(refused[1], "sync");
    assert!(
        refused[2].as_str().unwrap().starts_with("auth-required:"),
        "{refused}"
    );
}

#[tokio::test]
async fn a_closed_subscription_hears_nothing_more() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let host = relay.canonical_host();
    let mut reader = server.connect(&host).await;
    reader.auth(&owner, server.now()).await;
    reader.req("live", json!({"kinds": [1]})).await;

    reader.send(json!(["CLOSE", "live"])).await;
    // A round trip after the CLOSE means the relay has acted on it.
    reader.req("other", json!({"kinds": [7]})).await;

    let mut writer = server.connect(&host).await;
    writer.auth(&owner, server.now()).await;
    let event = sign_event(&owner, 1, "unheard", &[], server.now());
    assert!(writer.publish(&event).await.0);
    assert!(reader.is_quiet().await);
}

#[tokio::test]
async fn events_with_a_bad_id_or_signature_are_refused() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&owner, server.now()).await;
    let event = sign_event(&owner, 1, "genuine", &[], server.now());

    let mut tampered = serde_json::to_value(&event).unwrap();
    tampered["content"] = json!("tampered");
    let mut unsigned = serde_json::to_value(&event).unwrap();
    unsigned["sig"] = json!("0".repeat(128));
    for forged in [tampered, unsigned] {
        client.send(json!(["EVENT", forged])).await;
        let ok = client.recv().await.unwrap();
        assert_eq!(ok[1], event.id.to_hex());
        assert_eq!(ok[2], false);
        assert!(ok[3].as_str().unwrap().starts_with("invalid:"), "{ok}");
    }

    let (stored, _) = client.req("sub", json!({"kinds": [1]})).await;
    assert!(stored.is_empty());
}

#[tokio::test]
async fn a_count_is_admitted_and_filtered_like_a_subscription() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let host = relay.canonical_host();
    let (insider, outsider) = (SecretKey::generate(), SecretKey::generate());
    for member in [&insider, &outsider] {
        caravel_core::management::add_member(&server.core, &relay, &member.public_key().to_hex())
            .await
            .unwrap();
    }
    server.relays.reload(&relay.id).await.unwrap();

    let mut admin = server.connect(&host).await;
    admin.auth(&owner, server.now()).await;
    let create = sign_event(
        &owner,
        9007,
        "",
        &[&["h", "secret"], &["private"]],
        server.now(),
    );
    assert!(admin.publish(&create).await.0);
    for i in 0..3 {
        let note = sign_event(&owner, 1, &i.to_string(), &[], server.now());
        assert!(admin.publish(&note).await.0);
    }
    let mut inside = server.connect(&host).await;
    inside.auth(&insider, server.now()).await;
    let join = sign_event(&insider, 9021, "", &[&["h", "secret"]], server.now());
    assert!(inside.publish(&join).await.0);
    for i in 0..2 {
        let post = sign_event(
            &insider,
            9,
            &i.to_string(),
            &[&["h", "secret"]],
            server.now(),
        );
        assert!(inside.publish(&post).await.0);
    }

    // A private room's posts count only for its members.
    let filter = json!({"kinds": [1, 9]});
    assert_eq!(
        count(&mut inside, vec![filter.clone()]).await,
        json!(["COUNT", "c", {"count": 5}])
    );
    let mut outside = server.connect(&host).await;
    outside.auth(&outsider, server.now()).await;
    assert_eq!(
        count(&mut outside, vec![filter.clone()]).await,
        json!(["COUNT", "c", {"count": 3}])
    );

    // Filters are a union, each event counted once, and a limit doesn't cap a count.
    let union = vec![
        json!({"kinds": [1]}),
        json!({"authors": [owner.public_key().to_hex()], "kinds": [1], "limit": 1}),
    ];
    assert_eq!(
        count(&mut inside, union).await,
        json!(["COUNT", "c", {"count": 3}])
    );

    // A reader who may not subscribe may not count either.
    let mut stranger = server.connect(&host).await;
    stranger.auth(&SecretKey::generate(), server.now()).await;
    assert_eq!(
        count(&mut stranger, vec![filter.clone()]).await,
        json!([
            "CLOSED",
            "c",
            "restricted: you are not a member of this relay"
        ])
    );
    let mut anonymous = server.connect(&host).await;
    anonymous.send(json!(["COUNT", "c", filter])).await;
    assert_eq!(
        anonymous.recv().await.unwrap(),
        json!(["AUTH", anonymous.challenge])
    );
    let refused = anonymous.recv().await.unwrap();
    assert_eq!(refused[0], "CLOSED");
    assert!(
        refused[2].as_str().unwrap().starts_with("auth-required:"),
        "{refused}"
    );
}

/// Ask for a count, returning the relay's answer.
async fn count(client: &mut Client, filters: Vec<Value>) -> Value {
    let mut message = vec![json!("COUNT"), json!("c")];
    message.extend(filters);
    client.send(Value::Array(message)).await;
    client.recv_skipping_auth().await.unwrap()
}

#[tokio::test]
async fn a_request_holds_at_most_max_filters_filters() {
    let core = testing::core(&SecretKey::generate().public_key().to_hex()).await;
    let config = RelaysConfig {
        limits: RelayLimits {
            max_filters: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    let server = Server::start_configured(core, config).await;
    let (relay, owner) = server.relay().await;
    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&owner, server.now()).await;

    let refused = json!([
        "CLOSED",
        "c",
        "blocked: a request may have at most 2 filters"
    ]);
    let mut req = vec![json!("REQ"), json!("c")];
    req.extend(vec![json!({"kinds": [1]}); 3]);
    client.send(Value::Array(req)).await;
    assert_eq!(client.recv().await.unwrap(), refused);
    assert_eq!(
        count(&mut client, vec![json!({"kinds": [1]}); 3]).await,
        refused
    );
    assert_eq!(
        count(&mut client, vec![json!({"kinds": [1]}); 2]).await,
        json!(["COUNT", "c", {"count": 0}])
    );
}

#[tokio::test]
async fn a_change_published_before_watching_starts_is_applied() {
    let core = testing::core(&SecretKey::generate().public_key().to_hex()).await;
    let shutdown = CancellationToken::new();
    let relays = Relays::load(
        core.clone(),
        RelaysConfig::default(),
        shutdown.child_token(),
    )
    .await
    .unwrap();

    let (relay, _) = testing::create_relay(&core).await;
    tokio::spawn({
        let (relays, shutdown) = (relays.clone(), shutdown.clone());
        async move { relays.watch(shutdown).await }
    });

    let served = tokio::time::timeout(Duration::from_secs(5), async {
        while relays.resolve(&relay.canonical_host()).is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    shutdown.cancel();
    assert!(served.is_ok(), "the new relay was never served");
}

#[tokio::test]
async fn a_limit_of_zero_serves_nothing_stored_but_listens() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let host = relay.canonical_host();
    let mut reader = server.connect(&host).await;
    reader.auth(&owner, server.now()).await;
    let old = sign_event(&owner, 1, "old", &[], server.now());
    assert!(reader.publish(&old).await.0);

    let (stored, eose) = reader.req("live", json!({"kinds": [1], "limit": 0})).await;
    assert!(stored.is_empty());
    assert_eq!(eose[0], "EOSE");

    let mut writer = server.connect(&host).await;
    writer.auth(&owner, server.now()).await;
    let new = sign_event(&owner, 1, "new", &[], server.now());
    assert!(writer.publish(&new).await.0);
    assert_eq!(reader.recv().await.unwrap()[2]["id"], new.id.to_hex());
}

#[tokio::test]
async fn several_filters_are_answered_together_without_duplicates() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&owner, server.now()).await;
    let note = sign_event(&owner, 1, "note", &[&["t", "x"]], server.now() - 1);
    let reaction = sign_event(&owner, 7, "+", &[], server.now());
    assert!(client.publish(&note).await.0);
    assert!(client.publish(&reaction).await.0);

    client
        .send(json!(["REQ", "sub", {"kinds": [1]}, {"#t": ["x"]}, {"kinds": [7]}]))
        .await;
    let mut served = Vec::new();
    loop {
        let message = client.recv().await.unwrap();
        if message[0] == "EOSE" {
            break;
        }
        served.push(message[2]["id"].as_str().unwrap().to_string());
    }
    served.sort();
    let mut expected = vec![note.id.to_hex(), reaction.id.to_hex()];
    expected.sort();
    assert_eq!(served, expected);
}

#[tokio::test]
async fn ephemeral_events_are_delivered_but_not_stored() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let host = relay.canonical_host();
    let mut reader = server.connect(&host).await;
    reader.auth(&owner, server.now()).await;
    reader.req("live", json!({"kinds": [20001]})).await;

    let mut writer = server.connect(&host).await;
    writer.auth(&owner, server.now()).await;
    let event = sign_event(&owner, 20001, "now or never", &[], server.now());
    assert!(writer.publish(&event).await.0);
    assert_eq!(reader.recv().await.unwrap()[2]["id"], event.id.to_hex());

    let (stored, _) = writer.req("stored", json!({"kinds": [20001]})).await;
    assert!(stored.is_empty());
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

    assert!(client.is_dropped().await, "the connection is closed");
    assert!(server.relays.resolve(&host).is_none());
}

#[tokio::test]
async fn a_verified_custom_domain_and_the_canonical_host_both_serve_the_relay() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    server
        .update(&relay, json!({"custom_domain": "chat.example.com"}))
        .await;
    caravel_db::commands::relays::set_custom_domain_verified(
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
    assert_eq!(custom.primary_host(), "chat.example.com");
    let canonical = server
        .relays
        .resolve(&relay.canonical_host())
        .expect("canonical host not served");
    assert_eq!(canonical.primary_host(), "chat.example.com");

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
async fn a_custom_domain_stays_with_the_relay_that_verified_it() {
    let server = Server::start().await;
    let domain = "shared.example.com";
    let (first, _) = server.relay().await;
    let (second, _) = server.relay().await;
    let first = server
        .update(&first, json!({"custom_domain": domain}))
        .await;
    let second = server
        .update(&second, json!({"custom_domain": domain}))
        .await;

    let verify = async |relay: &caravel_domain::relays::Relay| {
        caravel_db::commands::relays::set_custom_domain_verified(
            server.core.db.pool(),
            &relay.id,
            domain,
            true,
        )
        .await
        .unwrap()
    };
    assert!(verify(&first).await);
    // Verifying another relay for it neither takes the domain nor fails.
    assert!(!verify(&second).await);
    server.relays.reload(&first.id).await.unwrap();
    let route = server
        .relays
        .resolve(domain)
        .expect("custom domain not served");
    assert_eq!(route.instance.relay_id(), first.id);

    // The second keeps its claim through other changes, and nobody can make a new one.
    server
        .update(&second, json!({"info_name": "pending"}))
        .await;
    let (third, _) = server.relay().await;
    let settings = serde_json::from_value(json!({"custom_domain": domain})).unwrap();
    let taken = caravel_core::hosting::update_relay(&server.core, third, settings).await;
    assert!(matches!(
        taken,
        Err(caravel_util::error::Error::Invalid {
            code: "domain-exists",
            ..
        })
    ));
}

// Naming a domain proves nothing, so another relay that names it can't take the
// redirect a relay keeps from its old domain.
#[tokio::test]
async fn an_old_domain_keeps_redirecting_when_another_relay_names_it() {
    let server = Server::start().await;
    let old = "old.example.com";
    let pool = server.core.db.pool();
    let (first, _) = server.relay().await;
    let (second, _) = server.relay().await;

    let first = server.update(&first, json!({"custom_domain": old})).await;
    assert!(
        caravel_db::commands::relays::set_custom_domain_verified(pool, &first.id, old, true)
            .await
            .unwrap()
    );
    let first = caravel_db::queries::relays::require_relay(pool, &first.id)
        .await
        .unwrap();
    let first = server
        .update(&first, json!({"custom_domain": "new.example.com"}))
        .await;
    assert_eq!(first.redirect_from, [old]);

    server.update(&second, json!({"custom_domain": old})).await;
    let first = caravel_db::queries::relays::require_relay(pool, &first.id)
        .await
        .unwrap();
    assert_eq!(first.redirect_from, [old]);
    server.relays.reload(&first.id).await.unwrap();
    let route = server.relays.resolve(old).expect("old domain not served");
    assert_eq!(route.instance.relay_id(), first.id);
}

#[tokio::test]
async fn renaming_a_relay_withdraws_its_custom_domains_verification() {
    let server = Server::start().await;
    let domain = "renamed.example.com";
    let (relay, _) = server.relay().await;
    let relay = server
        .update(&relay, json!({"custom_domain": domain}))
        .await;
    caravel_db::commands::relays::set_custom_domain_verified(
        server.core.db.pool(),
        &relay.id,
        domain,
        true,
    )
    .await
    .unwrap();
    server.relays.reload(&relay.id).await.unwrap();
    assert!(server.relays.resolve(domain).is_some());

    let subdomain = caravel_core::testing::subdomain();
    let renamed = server.update(&relay, json!({"subdomain": subdomain})).await;
    assert!(!renamed.custom_domain_verified);
    assert!(server.relays.resolve(domain).is_none());
    assert!(server.relays.resolve(&renamed.canonical_host()).is_some());
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

#[tokio::test]
async fn a_request_to_vanish_must_name_a_host_the_relay_answers_on() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let relay = server
        .update(&relay, json!({"custom_domain": "unverified.example"}))
        .await;

    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&owner, server.now()).await;

    let elsewhere = sign_event(
        &owner,
        kinds::VANISH,
        "",
        &[&["relay", "wss://unverified.example/"]],
        server.now(),
    );
    let (accepted, message) = client.publish(&elsewhere).await;
    assert!(!accepted);
    assert!(message.contains("names another relay"), "{message}");

    let here = sign_event(
        &owner,
        kinds::VANISH,
        "",
        &[&["relay", &format!("wss://{}:443/", relay.canonical_host())]],
        server.now(),
    );
    assert!(client.publish(&here).await.0);
}

#[tokio::test]
async fn a_browser_may_preflight_any_relay_endpoint() {
    let server = Server::start().await;
    let (relay, _) = server.relay().await;

    let preflight = server
        .http(
            Request::builder()
                .method("OPTIONS")
                .uri("/upload")
                .header("host", relay.canonical_host())
                .header("origin", "https://app.example")
                .header("access-control-request-method", "PUT")
                .header("access-control-request-headers", "authorization, x-sha-256")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(preflight.status, StatusCode::OK);
    assert_eq!(preflight.header("access-control-allow-origin"), Some("*"));
    let allowed = preflight.header("access-control-allow-headers").unwrap();
    assert!(
        allowed.contains("authorization") && allowed.contains("x-sha-256"),
        "{allowed}"
    );
    assert!(
        preflight
            .header("access-control-allow-methods")
            .unwrap()
            .contains("PUT")
    );
}
