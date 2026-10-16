//! Calls, ported from zooid's LiveKit tests: tokens for members of rooms with
//! calls, and the webhook that meters call time and publishes who is in a call.

mod common;

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use caravel_adapters::livekit::Livekit;
use caravel_core::calls;
use caravel_core::relays::Relay;
use caravel_core::testing::{self, sign_event};
use common::*;
use coracle_lib::keys::SecretKey;
use serde_json::{Value, json};

/// A LiveKit room service that creates any room and lists `participants`.
async fn fake_livekit() -> (String, Arc<Mutex<Vec<String>>>) {
    let participants: Arc<Mutex<Vec<String>>> = Arc::default();
    let router = Router::new()
        .route(
            "/twirp/livekit.RoomService/CreateRoom",
            post(|| async { Json(json!({})) }),
        )
        .route(
            "/twirp/livekit.RoomService/ListParticipants",
            post(
                |State(participants): State<Arc<Mutex<Vec<String>>>>| async move {
                    let listed: Vec<Value> = participants
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|identity| json!({"identity": identity, "sid": "PA_x"}))
                        .collect();
                    Json(json!({"participants": listed}))
                },
            ),
        )
        .with_state(participants.clone());
    let addr = serve_local(router).await;
    (format!("ws://{addr}"), participants)
}

async fn start() -> (Server, Arc<Mutex<Vec<String>>>, String) {
    let (url, participants) = fake_livekit().await;
    let mut core = testing::core(&SecretKey::generate().public_key().to_hex()).await;
    core.livekit = Some(Livekit::new(&url, "test-key", "test-secret").unwrap());
    (Server::start_with(core).await, participants, url)
}

/// A relay with calls, and rooms: "call" with calls, "restricted" with calls for
/// its members, and "quiet" without.
async fn call_relay(server: &Server) -> (Relay, SecretKey) {
    let (relay, owner) = server.relay().await;
    let relay = server
        .update(&relay, json!({"livekit_enabled": 1, "plan_id": "basic"}))
        .await;

    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&owner, server.now()).await;
    for tags in [
        vec![vec!["h", "call"], vec!["livekit"]],
        vec![vec!["h", "restricted"], vec!["livekit"], vec!["restricted"]],
        vec![vec!["h", "quiet"]],
    ] {
        let tags: Vec<&[&str]> = tags.iter().map(Vec::as_slice).collect();
        let create = sign_event(&owner, 9007, "", &tags, server.now());
        assert!(client.publish(&create).await.0);
    }
    (relay, owner)
}

async fn token(
    server: &Server,
    relay: &Relay,
    signer: Option<&SecretKey>,
    room: &str,
) -> HttpResponse {
    let path = format!("/.well-known/nip29/livekit/{room}");
    let mut request = Request::get(&path).header("host", relay.canonical_host());
    if let Some(signer) = signer {
        let url = format!("https://{}{path}", relay.canonical_host());
        let auth = sign_event(
            signer,
            27235,
            "",
            &[&["u", &url], &["method", "GET"]],
            server.now(),
        );
        request = request.header("authorization", nostr_authorization(&auth));
    }
    server.http(request.body(Body::empty()).unwrap()).await
}

fn claims(token: &str) -> Value {
    use base64::Engine;
    let payload = token.split('.').nth(1).unwrap();
    serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn a_relay_says_whether_it_has_calls() {
    let (server, _, _) = start().await;
    let (relay, _) = call_relay(&server).await;
    let (plain, _) = server.relay().await;

    let support = |relay: &Relay| {
        Request::get("/.well-known/nip29/livekit")
            .header("host", relay.canonical_host())
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        server.http(support(&relay)).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        server.http(support(&plain)).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_member_gets_a_token_for_a_rooms_call() {
    let (server, _, url) = start().await;
    let (relay, owner) = call_relay(&server).await;

    let response = token(&server, &relay, Some(&owner), "call").await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let body = response.json();
    assert_eq!(body["server_url"], url);

    let claims = claims(body["participant_token"].as_str().unwrap());
    assert_eq!(claims["iss"], "test-key");
    assert_eq!(
        claims["video"]["room"],
        calls::livekit_room(&relay.id, "call")
    );
    assert_eq!(claims["video"]["roomJoin"], true);
    assert!(
        claims["sub"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{}:", owner.public_key().to_hex()))
    );
}

#[tokio::test]
async fn tokens_are_refused_to_those_who_may_not_join() {
    let (server, _, _) = start().await;
    let (relay, owner) = call_relay(&server).await;
    let member = SecretKey::generate();
    server
        .manage(
            &relay.canonical_host(),
            &owner,
            "allowpubkey",
            json!([member.public_key().to_hex()]),
        )
        .await;

    let cases = [
        (None, "call", StatusCode::UNAUTHORIZED),
        (Some(SecretKey::generate()), "call", StatusCode::FORBIDDEN),
        (Some(owner.clone()), "nowhere", StatusCode::NOT_FOUND),
        (Some(owner.clone()), "quiet", StatusCode::FORBIDDEN),
        (Some(member.clone()), "restricted", StatusCode::FORBIDDEN),
        (Some(member.clone()), "call", StatusCode::OK),
        (Some(owner.clone()), "restricted", StatusCode::OK),
    ];
    for (signer, room, status) in cases {
        let response = token(&server, &relay, signer.as_ref(), room).await;
        assert_eq!(response.status, status, "{room}: {:?}", response.body);
    }
}

async fn webhook(server: &Server, relay: &Relay, livekit: &Livekit, body: Value) -> HttpResponse {
    let body = body.to_string();
    let signature = livekit.sign_webhook(body.as_bytes(), server.now()).unwrap();
    server
        .http(
            Request::post("/.well-known/nip29/livekit/webhook")
                .header("host", relay.canonical_host())
                .header("authorization", signature)
                .header("content-type", "application/webhook+json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
}

#[tokio::test]
async fn a_departure_is_metered_once_and_the_call_published() {
    let (server, participants, url) = start().await;
    let (relay, owner) = call_relay(&server).await;
    let livekit = Livekit::new(&url, "test-key", "test-secret").unwrap();
    let staying = SecretKey::generate().public_key().to_hex();
    participants
        .lock()
        .unwrap()
        .push(format!("{staying}:nonce"));

    let room = calls::livekit_room(&relay.id, "call");
    let left = json!({
        "event": "participant_left",
        "room": {"name": room},
        "participant": {"sid": "PA_left", "identity": format!("{}:n", owner.public_key().to_hex()), "joinedAt": "1000"},
        "createdAt": "1060",
    });
    for _ in 0..2 {
        assert_eq!(
            webhook(&server, &relay, &livekit, left.clone())
                .await
                .status,
            StatusCode::NO_CONTENT
        );
    }
    assert_eq!(
        calls::participant_seconds(server.core.db.pool(), &relay.id)
            .await
            .unwrap(),
        60
    );

    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&owner, server.now()).await;
    let (listed, _) = client
        .req("call", json!({"kinds": [39004], "#d": ["call"]}))
        .await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["pubkey"], relay.pubkey);
    assert_eq!(
        listed[0]["tags"],
        json!([["d", "call"], ["participant", staying]])
    );

    // Once the room closes, nobody is in the call.
    server.core.clock.pin(server.now() + 1);
    let finished = json!({"event": "room_finished", "room": {"name": room}});
    assert_eq!(
        webhook(&server, &relay, &livekit, finished).await.status,
        StatusCode::NO_CONTENT
    );
    let (listed, _) = client
        .req("after", json!({"kinds": [39004], "#d": ["call"]}))
        .await;
    assert_eq!(listed[0]["tags"], json!([["d", "call"]]));
}

#[tokio::test]
async fn a_webhook_must_be_signed_and_name_a_known_call() {
    let (server, _, url) = start().await;
    let (relay, _) = call_relay(&server).await;
    let livekit = Livekit::new(&url, "test-key", "test-secret").unwrap();
    let forger = Livekit::new(&url, "test-key", "wrong-secret").unwrap();

    let joined = |room: String| json!({"event": "participant_joined", "room": {"name": room}});
    let room = calls::livekit_room(&relay.id, "call");

    assert_eq!(
        webhook(&server, &relay, &forger, joined(room.clone()))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        webhook(
            &server,
            &relay,
            &livekit,
            joined("missing:call".to_string())
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        webhook(
            &server,
            &relay,
            &livekit,
            joined(calls::livekit_room(&relay.id, "quiet"))
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        webhook(&server, &relay, &livekit, joined(room))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
}
