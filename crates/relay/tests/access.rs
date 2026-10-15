//! Access over the wire: joining and leaving with NIP-43, managing with NIP-86,
//! rooms with NIP-29, and members added through the API reaching the relay.

mod common;

use caravel_core::hosting;
use caravel_core::management::{self, Call};
use caravel_core::testing::sign_event;
use common::*;
use coracle_lib::keys::SecretKey;
use serde_json::json;

const JOIN: u16 = 28934;
const LEAVE: u16 = 28936;

#[tokio::test]
async fn a_claim_lets_a_stranger_join_and_read() {
    let server = Server::start().await;
    let (relay, _) = server.relay().await;
    management::call(&server.core, &relay, Call::CreateClaim("welcome".into()))
        .await
        .unwrap();
    let stranger = SecretKey::generate();

    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&stranger, server.now()).await;

    let bare = sign_event(&stranger, JOIN, "", &[], server.now());
    assert_eq!(
        client.publish(&bare).await,
        (false, "restricted: no claim tag".to_string())
    );

    let wrong = sign_event(&stranger, JOIN, "", &[&["claim", "nope"]], server.now());
    let (accepted, message) = client.publish(&wrong).await;
    assert!(!accepted);
    assert_eq!(message, "restricted: failed to validate invite code");

    let join = sign_event(&stranger, JOIN, "", &[&["claim", "welcome"]], server.now());
    assert!(client.publish(&join).await.0);

    let (_, eose) = client.req("sub", json!({"kinds": [1]})).await;
    assert_eq!(eose[0], "EOSE", "a member may read");

    let log = caravel_core::access::get_claim_log(server.core.db.pool(), &relay.id, "welcome")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(log.uses[0].pubkey, stranger.public_key().to_hex());
}

#[tokio::test]
async fn an_open_relay_takes_joins_up_to_its_plans_member_limit() {
    let server = Server::start().await;
    let (relay, _) = server.relay().await;
    let relay = server
        .update(&relay, json!({"policy_public_join": 1}))
        .await;

    // The free plan holds ten members, the owner among them.
    for _ in 0..9 {
        hosting::add_member(
            &server.core,
            &relay,
            &SecretKey::generate().public_key().to_hex(),
        )
        .await
        .unwrap();
    }
    server.relays.reload(&relay.id).await.unwrap();

    let latecomer = SecretKey::generate();
    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&latecomer, server.now()).await;
    let join = sign_event(&latecomer, JOIN, "", &[], server.now());
    assert_eq!(
        client.publish(&join).await,
        (
            false,
            "restricted: this relay has reached its member limit".to_string()
        )
    );
}

#[tokio::test]
async fn leaving_takes_away_access() {
    let server = Server::start().await;
    let (relay, _) = server.relay().await;
    let relay = server
        .update(&relay, json!({"policy_public_join": 1}))
        .await;
    let member = SecretKey::generate();

    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&member, server.now()).await;
    assert!(
        client
            .publish(&sign_event(&member, JOIN, "", &[], server.now()))
            .await
            .0
    );
    assert!(
        client
            .publish(&sign_event(&member, LEAVE, "", &[], server.now()))
            .await
            .0
    );

    let (_, closed) = client.req("sub", json!({"kinds": [1]})).await;
    assert_eq!(closed[2], "restricted: you are not a member of this relay");
}

#[tokio::test]
async fn management_calls_are_authorized_per_method() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let host = relay.canonical_host();
    let stranger = SecretKey::generate();

    let methods = server
        .manage(&host, &owner, "supportedmethods", json!([]))
        .await;
    assert_eq!(
        methods["result"].as_array().unwrap().len(),
        caravel_core::access::MANAGED_METHODS.len()
    );

    let refused = server
        .manage(&host, &stranger, "supportedmethods", json!([]))
        .await;
    assert_eq!(
        refused["error"],
        "blocked: only relay admins can manage this relay."
    );
    let refused = server
        .manage(&host, &stranger, "listclaims", json!([]))
        .await;
    assert_eq!(
        refused["error"],
        "blocked: you are not authorized to use this method"
    );

    assert_eq!(
        server
            .manage(&host, &owner, "createclaim", json!(["abc"]))
            .await["result"],
        true
    );
    assert_eq!(
        server.manage(&host, &owner, "listclaims", json!([])).await["result"],
        json!(["abc"])
    );

    let delegate = SecretKey::generate();
    let assigned = server
        .manage(
            &host,
            &owner,
            "assignmethod",
            json!([delegate.public_key().to_hex(), "banevent"]),
        )
        .await;
    assert_eq!(assigned["result"], true);
    let methods = server
        .manage(&host, &delegate, "supportedmethods", json!([]))
        .await;
    assert!(
        methods["result"]
            .as_array()
            .unwrap()
            .contains(&json!("banevent"))
    );
}

#[tokio::test]
async fn a_management_call_must_be_signed_over_its_body() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let host = relay.canonical_host();

    let auth = sign_event(
        &owner,
        27235,
        "",
        &[
            &["u", &format!("https://{host}/")],
            &["payload", &"0".repeat(64)],
        ],
        server.now(),
    );
    let response = server
        .manage_with(
            &host,
            &auth,
            json!({"method": "listclaims", "params": []}).to_string(),
        )
        .await;
    assert_eq!(response["error"], "invalid auth event payload hash");
}

#[tokio::test]
async fn members_of_an_inviting_relay_may_create_claims() {
    let server = Server::start().await;
    let (relay, _) = server.relay().await;
    let host = relay.canonical_host();
    let member = SecretKey::generate();
    hosting::add_member(&server.core, &relay, &member.public_key().to_hex())
        .await
        .unwrap();
    server.relays.reload(&relay.id).await.unwrap();

    assert_eq!(
        server
            .manage(&host, &member, "createclaim", json!(["mine"]))
            .await["result"],
        true
    );

    server
        .update(&relay, json!({"members_can_invite": 0}))
        .await;
    let refused = server
        .manage(&host, &member, "createclaim", json!(["more"]))
        .await;
    assert_eq!(
        refused["error"],
        "blocked: you are not authorized to use this method"
    );
}

#[tokio::test]
async fn a_banned_member_stops_receiving_at_once() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let host = relay.canonical_host();
    let member = SecretKey::generate();
    let member_hex = member.public_key().to_hex();
    hosting::add_member(&server.core, &relay, &member_hex)
        .await
        .unwrap();
    server.relays.reload(&relay.id).await.unwrap();

    let mut reader = server.connect(&host).await;
    reader.auth(&member, server.now()).await;
    reader.req("live", json!({"kinds": [1]})).await;

    assert_eq!(
        server
            .manage(&host, &owner, "banpubkey", json!([member_hex, "spam"]))
            .await["result"],
        true
    );

    let mut writer = server.connect(&host).await;
    writer.auth(&owner, server.now()).await;
    assert!(
        writer
            .publish(&sign_event(&owner, 1, "after the ban", &[], server.now()))
            .await
            .0
    );
    assert!(reader.is_quiet().await, "the banned reader got nothing");

    let (accepted, message) = reader
        .publish(&sign_event(&member, 1, "still here?", &[], server.now()))
        .await;
    assert!(!accepted);
    assert_eq!(
        message,
        "blocked: event author has been banned from this relay"
    );
}

#[tokio::test]
async fn a_member_added_through_the_api_can_read_straight_away() {
    let server = Server::start().await;
    let (relay, _) = server.relay().await;
    let member = SecretKey::generate();

    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&member, server.now()).await;
    let (_, closed) = client.req("before", json!({"kinds": [1]})).await;
    assert_eq!(closed[0], "CLOSED");

    // The API's change reaches the relay over the bus.
    hosting::add_member(&server.core, &relay, &member.public_key().to_hex())
        .await
        .unwrap();
    let mut eose = serde_json::Value::Null;
    for _ in 0..20 {
        let (_, end) = client.req("after", json!({"kinds": [1]})).await;
        if end[0] == "EOSE" {
            eose = end;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(eose[0], "EOSE");
}

#[tokio::test]
async fn a_private_room_is_read_only_by_its_members_stored_and_live() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;
    let host = relay.canonical_host();
    let insider = SecretKey::generate();
    let outsider = SecretKey::generate();
    for member in [&insider, &outsider] {
        hosting::add_member(&server.core, &relay, &member.public_key().to_hex())
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

    let mut inside = server.connect(&host).await;
    inside.auth(&insider, server.now()).await;
    assert!(
        inside
            .publish(&sign_event(
                &insider,
                9021,
                "",
                &[&["h", "secret"]],
                server.now()
            ))
            .await
            .0
    );
    inside
        .req("room", json!({"#h": ["secret"], "kinds": [9]}))
        .await;

    let mut outside = server.connect(&host).await;
    outside.auth(&outsider, server.now()).await;
    outside
        .req("room", json!({"#h": ["secret"], "kinds": [9]}))
        .await;

    let message = sign_event(
        &insider,
        9,
        "for members",
        &[&["h", "secret"]],
        server.now(),
    );
    assert!(inside.publish(&message).await.0);

    let delivered = inside.recv_skipping_auth().await.unwrap();
    assert_eq!(delivered[2]["id"], message.id.to_hex());
    assert!(outside.is_quiet().await, "an outsider gets nothing live");

    let (stored, _) = outside.req("again", json!({"#h": ["secret"]})).await;
    assert!(
        stored.iter().all(|event| event["kind"] == 39000),
        "only the metadata shows: {stored:?}"
    );
    let (stored, _) = inside
        .req("again", json!({"#h": ["secret"], "kinds": [9]}))
        .await;
    assert_eq!(stored.len(), 1);
}
