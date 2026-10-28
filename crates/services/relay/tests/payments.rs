//! A relay that sells memberships, ported from zooid's payments tests: what its
//! information document says about paying, and who it lets join.

mod common;

use axum::body::Body;
use axum::http::Request;
use caravel_core::management;
use caravel_core::testing::sign_event;
use caravel_domain::management::Call;
use caravel_util::nostr::kinds;
use common::*;
use coracle_lib::keys::SecretKey;
use serde_json::json;

async fn info(server: &Server, host: &str) -> serde_json::Value {
    server
        .http(
            Request::get("/")
                .header("host", host)
                .header("accept", "application/nostr+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .json()
}

#[tokio::test]
async fn a_paid_relay_states_its_price_and_where_to_pay() {
    let server = Server::start().await;
    let (relay, _) = server.relay().await;

    let free = info(&server, &relay.canonical_host()).await;
    assert_eq!(free["limitation"]["payment_required"], false);
    assert!(free.get("payments_url").is_none());
    assert!(free.get("fees").is_none());

    let relay = server.update(&relay, json!({"member_price": 500})).await;
    let paid = info(&server, &relay.canonical_host()).await;
    assert_eq!(paid["limitation"]["payment_required"], true);
    assert_eq!(paid["payments_url"], format!("https://{}/", relay.host()));
    assert_eq!(
        paid["fees"],
        json!({"subscription": [{"amount": 500, "unit": "USD", "period": 2592000}]})
    );

    let relay = server
        .update(&relay, json!({"member_interval": "year"}))
        .await;
    let yearly = info(&server, &relay.canonical_host()).await;
    assert_eq!(yearly["fees"]["subscription"][0]["period"], 31536000);
}

#[tokio::test]
async fn a_paid_relay_takes_joins_only_with_a_claim() {
    let server = Server::start().await;
    let (relay, _) = server.relay().await;
    let relay = server.update(&relay, json!({"member_price": 500})).await;
    management::call(&server.core, &relay, Call::CreateClaim("comped".into()))
        .await
        .unwrap();
    let stranger = SecretKey::generate();

    let mut client = server.connect(&relay.canonical_host()).await;
    client.auth(&stranger, server.now()).await;

    let bare = sign_event(&stranger, kinds::RELAY_JOIN, "", &[], server.now());
    assert_eq!(
        client.publish(&bare).await,
        (
            false,
            "restricted: this space requires a paid membership".to_string()
        )
    );

    let comped = sign_event(
        &stranger,
        kinds::RELAY_JOIN,
        "",
        &[&["claim", "comped"]],
        server.now(),
    );
    assert!(client.publish(&comped).await.0);
}
