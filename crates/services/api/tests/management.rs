//! Managing a relay from the dashboards: NIP-86 methods, members and roles, rooms
//! moderated through the relay's key, and paging through its events.

mod common;

use axum::http::StatusCode;
use caravel_core::hosting;
use caravel_core::testing::{create_relay, sign_event};
use common::*;
use coracle_lib::keys::SecretKey;
use serde_json::{Value, json};

fn pubkey() -> String {
    SecretKey::generate().public_key().to_hex()
}

#[tokio::test]
async fn an_owner_calls_the_relays_methods_and_a_stranger_may_not() {
    let api = TestApi::new().await;
    let (relay, owner) = create_relay(&api.core).await;
    let (_, stranger) = create_relay(&api.core).await;
    let owner = api.bearer(&owner).await;
    let path = format!("/relays/{}/management", relay.id);
    let call = |method: &str, params: Value| json!({ "method": method, "params": params });

    let stranger = api.bearer(&stranger).await;
    let refused = api
        .request(
            "POST",
            &path,
            Some(&stranger),
            Some(call("listbannedpubkeys", json!([]))),
        )
        .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);

    let methods = api
        .request(
            "POST",
            &path,
            Some(&owner),
            Some(call("supportedmethods", json!([]))),
        )
        .await;
    assert!(
        methods.body["data"]
            .as_array()
            .unwrap()
            .contains(&json!("banpubkey"))
    );

    let spammer = pubkey();
    let banned = api
        .request(
            "POST",
            &path,
            Some(&owner),
            Some(call("banpubkey", json!([spammer, "spam"]))),
        )
        .await;
    assert_eq!(banned.status, StatusCode::OK);
    let bans = api
        .request(
            "POST",
            &path,
            Some(&owner),
            Some(call("listbannedpubkeys", json!([]))),
        )
        .await;
    assert_eq!(
        bans.body["data"],
        json!([{ "pubkey": spammer, "reason": "spam" }])
    );

    // Roles and members come back together.
    let member = pubkey();
    for (method, params) in [
        ("createrole", json!(["mod", "Moderator", "", 30, 1])),
        ("allowpubkey", json!([member])),
        ("assignrole", json!([member, "mod"])),
    ] {
        let response = api
            .request("POST", &path, Some(&owner), Some(call(method, params)))
            .await;
        assert_eq!(response.status, StatusCode::OK, "{method}");
    }
    let members = api
        .request(
            "GET",
            &format!("/relays/{}/members", relay.id),
            Some(&owner),
            None,
        )
        .await;
    assert_eq!(members.body["data"]["roles"][0]["id"], "mod");
    let listed = members.body["data"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|listed| listed["pubkey"] == member)
        .unwrap()
        .clone();
    assert_eq!(listed["roles"], json!(["mod"]));
    assert_eq!(listed["source"], "manual");

    let unknown = api
        .request(
            "POST",
            &path,
            Some(&owner),
            Some(call("nosuchmethod", json!([]))),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_purged_relays_members_are_out_of_reach() {
    let api = TestApi::new().await;
    let (relay, owner) = create_relay(&api.core).await;
    let owner = api.bearer(&owner).await;
    hosting::purge_relay(&api.core, &relay).await.unwrap();

    let member = format!("/relays/{}/members/{}", relay.id, pubkey());
    for (method, path) in [
        ("GET", format!("/relays/{}/members", relay.id)),
        ("PUT", member.clone()),
        ("DELETE", member),
    ] {
        let response = api.request(method, &path, Some(&owner), None).await;
        assert_eq!(response.status, StatusCode::BAD_REQUEST, "{method} {path}");
        assert_eq!(response.body["code"], "relay-is-purged", "{method} {path}");
    }
}

#[tokio::test]
async fn rooms_are_moderated_through_the_relays_key() {
    let api = TestApi::new().await;
    let (relay, owner) = create_relay(&api.core).await;
    let owner = api.bearer(&owner).await;
    let moderate = format!("/relays/{}/rooms/moderate", relay.id);
    let moderator = pubkey();

    for (kind, tags) in [
        (9007, json!([["h", "lobby"]])),
        (9002, json!([["h", "lobby"], ["name", "Lobby"], ["closed"]])),
        (9000, json!([["h", "lobby"], ["p", moderator, "host"]])),
        (9011, json!([["h", "lobby"], ["p", moderator, "9005"]])),
        (9009, json!([["h", "lobby"], ["code", "sesame"]])),
    ] {
        let response = api
            .request(
                "POST",
                &moderate,
                Some(&owner),
                Some(json!({ "kind": kind, "tags": tags })),
            )
            .await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{kind}: {:?}",
            response.body
        );
    }

    let rooms = api
        .request(
            "GET",
            &format!("/relays/{}/rooms", relay.id),
            Some(&owner),
            None,
        )
        .await;
    let lobby = &rooms.body["data"][0];
    assert_eq!(lobby["id"], "lobby");
    assert!(
        lobby["tags"]
            .as_array()
            .unwrap()
            .contains(&json!(["name", "Lobby"]))
    );
    assert_eq!(lobby["invites"], json!(["sesame"]));
    assert_eq!(lobby["members"][0]["pubkey"], moderator);
    assert_eq!(lobby["members"][0]["roles"], json!(["host"]));
    assert_eq!(lobby["members"][0]["permissions"], json!([9005]));

    // The relay signs what it's asked to, but the room rules still apply.
    for (kind, tags) in [
        (1, json!([["h", "lobby"]])),
        (9007, json!([["h", "lobby"]])),
        (9001, json!([["h", "nowhere"], ["p", moderator]])),
    ] {
        let response = api
            .request(
                "POST",
                &moderate,
                Some(&owner),
                Some(json!({ "kind": kind, "tags": tags })),
            )
            .await;
        assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY, "{kind}");
    }
}

#[tokio::test]
async fn a_relays_events_are_paged_by_kind_and_id() {
    let api = TestApi::new().await;
    let (relay, owner) = create_relay(&api.core).await;
    let bearer = api.bearer(&owner).await;
    let now = api.core.clock.now();

    let note = sign_event(&owner, 1, "hello", &[], now - 10);
    let report = sign_event(
        &SecretKey::generate(),
        1984,
        "",
        &[&["e", &note.id.to_hex(), "spam"]],
        now - 5,
    );
    for event in [&note, &report] {
        api.core
            .db
            .with_tx(async |conn| {
                caravel_relays::events::commands::save_event(conn, &relay.id, event, now).await
            })
            .await
            .unwrap();
    }

    let events = |query: String| {
        let api = &api;
        let bearer = &bearer;
        let path = format!("/relays/{}/events?{query}", relay.id);
        async move { api.request("GET", &path, Some(bearer), None).await }
    };

    let reports = events("kinds=1984".into()).await;
    assert_eq!(reports.body["data"].as_array().unwrap().len(), 1);
    assert_eq!(reports.body["data"][0]["id"], report.id.to_hex());

    let targets = events(format!("ids={}", note.id.to_hex())).await;
    assert_eq!(targets.body["data"][0]["content"], "hello");

    let older = events(format!("kinds=1,1984&until={}&limit=1", now - 6)).await;
    assert_eq!(older.body["data"].as_array().unwrap().len(), 1);
    assert_eq!(older.body["data"][0]["id"], note.id.to_hex());

    let bad = events("kinds=notes".into()).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn a_relays_events_page_past_a_shared_second_and_leave_out_its_own() {
    let api = TestApi::new().await;
    let (relay, owner) = create_relay(&api.core).await;
    let bearer = api.bearer(&owner).await;
    let now = api.core.clock.now();

    let mut notes: Vec<_> = (0..5)
        .map(|n| sign_event(&SecretKey::generate(), 1, &n.to_string(), &[], now - 10))
        .collect();
    let own = api.core.relay_key(&relay).unwrap().sign(
        8000,
        vec![vec!["p".to_string(), pubkey()]],
        "",
        now - 5,
    );
    for event in notes.iter().chain([&own]) {
        api.core
            .db
            .with_tx(async |conn| {
                caravel_relays::events::commands::save_event(conn, &relay.id, event, now).await
            })
            .await
            .unwrap();
    }
    notes.sort_by_key(|note| note.id.to_hex());
    let note_ids: Vec<String> = notes.iter().map(|note| note.id.to_hex()).collect();

    let ids = |query: String| {
        let api = &api;
        let bearer = &bearer;
        let path = format!("/relays/{}/events?{query}", relay.id);
        async move {
            let response = api.request("GET", &path, Some(bearer), None).await;
            assert_eq!(response.status, StatusCode::OK, "{query}");
            response.body["data"]
                .as_array()
                .unwrap()
                .iter()
                .map(|event| event["id"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        }
    };
    let next = |after: &str| format!("kinds=1&limit=2&until={}&after={after}", now - 10);

    assert_eq!(ids("kinds=1&limit=2".into()).await, note_ids[0..2]);
    assert_eq!(ids(next(&note_ids[1])).await, note_ids[2..4]);
    assert_eq!(ids(next(&note_ids[3])).await, note_ids[4..]);
    assert!(ids(next(&note_ids[4])).await.is_empty());

    assert!(ids(String::new()).await.contains(&own.id.to_hex()));
    let others = ids("exclude_own=true".into()).await;
    assert_eq!(others.len(), 5);
    assert!(!others.contains(&own.id.to_hex()));

    let path = format!("/relays/{}/events?until={now}&after=nope", relay.id);
    let bad = api.request("GET", &path, Some(&bearer), None).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
}
