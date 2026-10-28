//! Media over Blossom, ported from zooid's blossom tests: who may upload, list,
//! fetch and delete, and how blobs are served so they can't run on the relay's
//! origin.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use caravel_billing::relays::Relay;
use caravel_core::testing;
use caravel_core::testing::sign_event;
use caravel_util::nostr::kinds;
use common::*;
use coracle_lib::keys::SecretKey;
use serde_json::json;
use sha2::{Digest, Sha256};

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// A Blossom authorization for `verb`, covering `hashes`.
fn blossom_auth(server: &Server, signer: &SecretKey, verb: &str, hashes: &[&str]) -> String {
    let expiration = (server.now() + 60).to_string();
    let mut tags: Vec<Vec<&str>> = vec![vec!["t", verb], vec!["expiration", &expiration]];
    for hash in hashes {
        tags.push(vec!["x", hash]);
    }
    let tags: Vec<&[&str]> = tags.iter().map(Vec::as_slice).collect();
    testing::authorization(&sign_event(
        signer,
        kinds::BLOSSOM_AUTH,
        verb,
        &tags,
        server.now(),
    ))
}

async fn media_relay(server: &Server) -> (Relay, SecretKey) {
    let (relay, owner) = server.relay().await;
    let relay = server
        .update(&relay, json!({"blossom_enabled": 1, "plan_id": "basic"}))
        .await;
    (relay, owner)
}

async fn upload(
    server: &Server,
    relay: &Relay,
    signer: &SecretKey,
    bytes: &[u8],
    content_type: &str,
) -> HttpResponse {
    let hash = sha256(bytes);
    server
        .http(
            Request::put("/upload")
                .header("host", relay.canonical_host())
                .header("content-type", content_type)
                .header(
                    "authorization",
                    blossom_auth(server, signer, "upload", &[&hash]),
                )
                .body(Body::from(bytes.to_vec()))
                .unwrap(),
        )
        .await
}

async fn download(server: &Server, relay: &Relay, name: &str) -> HttpResponse {
    server
        .http(
            Request::get(format!("/{name}"))
                .header("host", relay.canonical_host())
                .body(Body::empty())
                .unwrap(),
        )
        .await
}

#[tokio::test]
async fn a_member_uploads_and_anyone_downloads() {
    let server = Server::start().await;
    let (relay, owner) = media_relay(&server).await;
    let png = b"\x89PNG not really";

    let uploaded = upload(&server, &relay, &owner, png, "image/png").await;
    assert_eq!(uploaded.status, StatusCode::OK, "{:?}", uploaded.body);
    let descriptor = uploaded.json();
    assert_eq!(descriptor["sha256"], sha256(png));
    assert_eq!(descriptor["size"], png.len());
    assert_eq!(descriptor["type"], "image/png");
    assert_eq!(
        descriptor["url"],
        format!("https://{}/{}.png", relay.canonical_host(), sha256(png))
    );

    let fetched = download(&server, &relay, &format!("{}.png", sha256(png))).await;
    assert_eq!(fetched.status, StatusCode::OK);
    assert_eq!(&fetched.body[..], png);
    assert_eq!(fetched.header("content-type"), Some("image/png"));
    assert_eq!(fetched.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(
        fetched.header("content-security-policy"),
        Some("default-src 'none'; sandbox")
    );
    assert_eq!(fetched.header("content-disposition"), None);
    assert_eq!(fetched.header("access-control-allow-origin"), Some("*"));
}

// Ported from zooid: a type a browser would run is served as a download.
#[tokio::test]
async fn unrenderable_blobs_are_served_as_downloads() {
    let server = Server::start().await;
    let (relay, owner) = media_relay(&server).await;
    let html = b"<script>alert(1)</script>";

    upload(&server, &relay, &owner, html, "text/html").await;
    let fetched = download(&server, &relay, &sha256(html)).await;
    assert_eq!(fetched.status, StatusCode::OK);
    assert_eq!(fetched.header("content-disposition"), Some("attachment"));
    assert_eq!(
        fetched.header("content-security-policy"),
        Some("default-src 'none'; sandbox")
    );
}

#[tokio::test]
async fn a_range_is_served_partially() {
    let server = Server::start().await;
    let (relay, owner) = media_relay(&server).await;
    let bytes = b"0123456789";
    upload(&server, &relay, &owner, bytes, "application/octet-stream").await;

    let fetched = server
        .http(
            Request::get(format!("/{}", sha256(bytes)))
                .header("host", relay.canonical_host())
                .header("range", "bytes=2-5")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(fetched.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(&fetched.body[..], b"2345");
    assert_eq!(fetched.header("content-range"), Some("bytes 2-5/10"));
}

// Ported from zooid: refusals carry their reason where browsers can read it.
#[tokio::test]
async fn outsiders_are_refused_with_a_reason() {
    let server = Server::start().await;
    let (relay, _) = media_relay(&server).await;

    let refused = upload(&server, &relay, &SecretKey::generate(), b"hi", "text/plain").await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(refused.header("x-reason"), Some("unauthorized"));
    assert_eq!(
        refused.header("access-control-expose-headers"),
        Some("x-reason")
    );
}

// Ported from zooid: a missing or malformed authorization is refused, not a crash.
#[tokio::test]
async fn a_delete_without_a_valid_authorization_is_refused() {
    let server = Server::start().await;
    let (relay, owner) = media_relay(&server).await;
    let bytes = b"keep me";
    upload(&server, &relay, &owner, bytes, "text/plain").await;

    for authorization in [None, Some("Nostr not-base64!"), Some("Nostr e30=")] {
        let mut request =
            Request::delete(format!("/{}", sha256(bytes))).header("host", relay.canonical_host());
        if let Some(authorization) = authorization {
            request = request.header("authorization", authorization);
        }
        let refused = server.http(request.body(Body::empty()).unwrap()).await;
        assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    }

    assert_eq!(
        download(&server, &relay, &sha256(bytes)).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn an_authorization_must_be_for_the_verb_and_unexpired() {
    let server = Server::start().await;
    let (relay, owner) = media_relay(&server).await;
    let bytes = b"hi";

    let wrong_verb = server
        .http(
            Request::put("/upload")
                .header("host", relay.canonical_host())
                .header(
                    "authorization",
                    blossom_auth(&server, &owner, "delete", &[&sha256(bytes)]),
                )
                .body(Body::from(bytes.to_vec()))
                .unwrap(),
        )
        .await;
    assert_eq!(wrong_verb.status, StatusCode::UNAUTHORIZED);

    let expired = testing::authorization(&sign_event(
        &owner,
        kinds::BLOSSOM_AUTH,
        "",
        &[
            &["t", "upload"],
            &["expiration", &(server.now() - 1).to_string()],
        ],
        server.now() - 100,
    ));
    let refused = server
        .http(
            Request::put("/upload")
                .header("host", relay.canonical_host())
                .header("authorization", expired)
                .body(Body::from(bytes.to_vec()))
                .unwrap(),
        )
        .await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);

    let uncovered = server
        .http(
            Request::put("/upload")
                .header("host", relay.canonical_host())
                .header(
                    "authorization",
                    blossom_auth(&server, &owner, "upload", &[&sha256(b"other")]),
                )
                .body(Body::from(bytes.to_vec()))
                .unwrap(),
        )
        .await;
    assert_eq!(uncovered.status, StatusCode::FORBIDDEN);
}

// BUD-11: a token naming servers is good only on those, so one made for another
// relay here can't be replayed on this one.
#[tokio::test]
async fn an_authorization_naming_servers_is_good_only_on_them() {
    let server = Server::start().await;
    let (relay, owner) = media_relay(&server).await;
    let bytes = b"scoped";
    let hash = sha256(bytes);
    let expiration = (server.now() + 60).to_string();
    let put = |named: &str| {
        let auth = testing::authorization(&sign_event(
            &owner,
            kinds::BLOSSOM_AUTH,
            "upload",
            &[
                &["t", "upload"],
                &["expiration", &expiration],
                &["x", &hash],
                &["server", named],
            ],
            server.now(),
        ));
        Request::put("/upload")
            .header("host", relay.canonical_host())
            .header("authorization", auth)
            .body(Body::from(bytes.to_vec()))
            .unwrap()
    };

    let elsewhere = server.http(put("other.relay.test")).await;
    assert_eq!(elsewhere.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        elsewhere.header("x-reason"),
        Some("the authorization is for another server")
    );

    let here = server.http(put(&relay.canonical_host())).await;
    assert_eq!(here.status, StatusCode::OK);
}

#[tokio::test]
async fn a_blob_stays_until_its_last_uploader_deletes_it() {
    let server = Server::start().await;
    let (relay, owner) = media_relay(&server).await;
    let member = SecretKey::generate();
    server
        .manage(
            &relay.canonical_host(),
            &owner,
            "allowpubkey",
            json!([member.public_key().to_hex()]),
        )
        .await;
    let bytes = b"shared";
    let hash = sha256(bytes);
    upload(&server, &relay, &owner, bytes, "text/plain").await;
    upload(&server, &relay, &member, bytes, "text/plain").await;

    let delete = |signer: &SecretKey| {
        Request::delete(format!("/{hash}"))
            .header("host", relay.canonical_host())
            .header(
                "authorization",
                blossom_auth(&server, signer, "delete", &[&hash]),
            )
            .body(Body::empty())
            .unwrap()
    };

    let stranger = SecretKey::generate();
    server
        .manage(
            &relay.canonical_host(),
            &owner,
            "allowpubkey",
            json!([stranger.public_key().to_hex()]),
        )
        .await;
    assert_eq!(
        server.http(delete(&stranger)).await.status,
        StatusCode::NOT_FOUND
    );

    assert_eq!(server.http(delete(&owner)).await.status, StatusCode::OK);
    assert_eq!(
        download(&server, &relay, &hash).await.status,
        StatusCode::OK
    );

    assert_eq!(server.http(delete(&member)).await.status, StatusCode::OK);
    assert_eq!(
        download(&server, &relay, &hash).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_member_lists_an_uploaders_blobs() {
    let server = Server::start().await;
    let (relay, owner) = media_relay(&server).await;
    upload(&server, &relay, &owner, b"one", "text/plain").await;

    let listed = server
        .http(
            Request::get(format!("/list/{}", owner.public_key().to_hex()))
                .header("host", relay.canonical_host())
                .header("authorization", blossom_auth(&server, &owner, "list", &[]))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.json()[0]["sha256"], sha256(b"one"));
}

#[tokio::test]
async fn a_relay_may_keep_its_media_to_members() {
    let server = Server::start().await;
    let (relay, owner) = media_relay(&server).await;
    let relay = server
        .update(&relay, json!({"blossom_authenticated_read": 1}))
        .await;
    let bytes = b"private";
    upload(&server, &relay, &owner, bytes, "text/plain").await;

    assert_eq!(
        download(&server, &relay, &sha256(bytes)).await.status,
        StatusCode::UNAUTHORIZED
    );

    let fetched = server
        .http(
            Request::get(format!("/{}", sha256(bytes)))
                .header("host", relay.canonical_host())
                .header("authorization", blossom_auth(&server, &owner, "get", &[]))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(fetched.status, StatusCode::OK);
}

#[tokio::test]
async fn a_relay_without_media_has_no_media_server() {
    let server = Server::start().await;
    let (relay, owner) = server.relay().await;

    let refused = upload(&server, &relay, &owner, b"hi", "text/plain").await;
    assert_eq!(refused.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_upload_is_checked_before_it_is_sent() {
    let server = Server::start().await;
    let (relay, owner) = media_relay(&server).await;

    let check = |size: u64| {
        Request::head("/upload")
            .header("host", relay.canonical_host())
            .header("x-content-length", size.to_string())
            .header(
                "authorization",
                blossom_auth(&server, &owner, "upload", &[]),
            )
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(server.http(check(1024)).await.status, StatusCode::OK);
    assert_eq!(
        server.http(check(11 * 1024 * 1024)).await.status,
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn a_member_mirrors_a_file_from_another_server() {
    let server = Server::start().await;
    let (relay, owner) = media_relay(&server).await;
    let bytes = b"mirrored bytes";
    let origin = serve_local(axum::Router::new().route(
        "/file",
        get(|| async { ([("content-type", "text/plain")], &b"mirrored bytes"[..]) }),
    ))
    .await;

    let mirrored = server
        .http(
            Request::put("/mirror")
                .header("host", relay.canonical_host())
                .header(
                    "authorization",
                    blossom_auth(&server, &owner, "upload", &[&sha256(bytes)]),
                )
                .body(Body::from(
                    json!({"url": format!("http://{origin}/file")}).to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(mirrored.status, StatusCode::OK, "{:?}", mirrored.body);
    assert_eq!(mirrored.json()["type"], "text/plain");
    assert_eq!(
        &download(&server, &relay, &sha256(bytes)).await.body[..],
        bytes
    );
}
