//! A relay's data through the API: exporting, importing and erasing its events,
//! deleting its tenant, and the admins' moderation feed and blob review.

mod common;

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::StatusCode;
use bytes::Bytes;
use caravel_core::hosting;
use caravel_core::testing::{create_relay, sign_event};
use common::*;
use coracle_lib::filters::Filter;
use coracle_lib::keys::SecretKey;
use http_body::{Frame, SizeHint};

#[tokio::test]
async fn an_owner_exports_one_relay_and_imports_into_another() {
    let api = TestApi::new().await;
    let (source, owner) = create_relay(&api.core).await;
    let bearer = api.bearer(&owner).await;
    let (target, target_owner) = create_relay(&api.core).await;
    let target_bearer = api.bearer(&target_owner).await;

    let note = sign_event(
        &SecretKey::generate(),
        1,
        "carried over",
        &[],
        api.core.clock.now(),
    );
    let now = api.core.clock.now();
    api.core
        .db
        .with_tx(async |conn| {
            caravel_relays::events::commands::save_event(conn, &source.id, &note, now).await
        })
        .await
        .unwrap();

    let (status, headers, body) = api
        .raw(
            "GET",
            &format!("/relays/{}/export", source.id),
            &bearer,
            Vec::new(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "application/x-ndjson");
    assert_eq!(
        headers["content-disposition"],
        format!("attachment; filename=\"{}.jsonl\"", source.subdomain)
    );
    assert!(String::from_utf8_lossy(&body).contains(&note.id.to_hex()));

    // Only the target's owner may import into it.
    let (status, _, _) = api
        .raw(
            "POST",
            &format!("/relays/{}/import", target.id),
            &bearer,
            body.to_vec(),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _, imported) = api
        .raw(
            "POST",
            &format!("/relays/{}/import", target.id),
            &target_bearer,
            body.to_vec(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let imported: serde_json::Value = serde_json::from_slice(&imported).unwrap();
    assert_eq!(imported["data"]["imported"], 1);
    assert_eq!(imported["data"]["invalid"], 0);
}

/// A body claiming a length it doesn't hold, as a client's Content-Length can.
struct Claimed(u64);

impl http_body::Body for Claimed {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        Poll::Ready(None)
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.0)
    }
}

#[tokio::test]
async fn an_import_over_a_gib_is_refused_before_it_is_read() {
    let api = TestApi::new().await;
    let (relay, owner) = create_relay(&api.core).await;
    let bearer = api.bearer(&owner).await;
    let path = format!("/relays/{}/import", relay.id);

    let (status, _, body) = api
        .raw("POST", &path, &bearer, Body::new(Claimed((1 << 30) + 1)))
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["code"], "import-too-large");

    let (status, _, _) = api
        .raw("POST", &path, &bearer, Body::new(Claimed(1 << 30)))
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_purged_relay_has_nothing_to_export() {
    let api = TestApi::new().await;
    let (relay, owner) = create_relay(&api.core).await;
    let bearer = api.bearer(&owner).await;
    hosting::purge_relay(&api.core, &relay).await.unwrap();

    let response = api
        .request(
            "GET",
            &format!("/relays/{}/export", relay.id),
            Some(&bearer),
            None,
        )
        .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_eq!(response.body["code"], "relay-is-purged");
}

#[tokio::test]
async fn an_owner_or_an_admin_erases_a_relay() {
    let api = TestApi::new().await;
    let (relay, owner) = create_relay(&api.core).await;
    let (_, stranger) = create_relay(&api.core).await;
    let path = format!("/relays/{}/erase", relay.id);

    let note = sign_event(&owner, 1, "erased", &[], api.core.clock.now());
    let now = api.core.clock.now();
    api.core
        .db
        .with_tx(async |conn| {
            caravel_relays::events::commands::save_event(conn, &relay.id, &note, now).await
        })
        .await
        .unwrap();

    let stranger = api.bearer(&stranger).await;
    let refused = api.request("POST", &path, Some(&stranger), None).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);

    for bearer in [api.bearer(&owner).await, api.bearer(&api.admin).await] {
        let erased = api.request("POST", &path, Some(&bearer), None).await;
        assert_eq!(erased.status, StatusCode::OK);
    }
    let filter = Filter {
        kinds: Some([1].into()),
        ..Filter::new()
    };
    assert!(
        caravel_relays::events::queries::query_events(
            api.core.db.pool(),
            &relay.id,
            &filter,
            10,
            now,
            &[]
        )
        .await
        .unwrap()
        .is_empty()
    );
}

#[tokio::test]
async fn an_admin_deletes_a_tenant_without_banning_them() {
    let api = TestApi::new().await;
    let (relay, owner) = create_relay(&api.core).await;
    let admin = api.bearer(&api.admin).await;
    let pubkey = owner.public_key().to_hex();

    let deleted = api
        .request(
            "POST",
            &format!("/tenants/{pubkey}/delete"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::OK);
    assert_eq!(deleted.body["data"]["banned_at"], serde_json::Value::Null);

    let relay = caravel_billing::relays::queries::require_relay(api.core.db.pool(), &relay.id)
        .await
        .unwrap();
    assert!(relay.is_purged());
}

#[tokio::test]
async fn admins_review_the_feed_and_its_blobs() {
    let api = TestApi::new().await;
    let (relay, owner) = create_relay(&api.core).await;
    let admin = api.bearer(&api.admin).await;
    let now = api.core.clock.now();

    let owner_pubkey = owner.public_key().to_hex();
    let blob = api
        .core
        .db
        .with_tx(async |conn| {
            caravel_relays::media::commands::record_upload(
                conn,
                &relay.id,
                &"a".repeat(64),
                "image/png",
                5,
                &owner_pubkey,
                now,
            )
            .await
        })
        .await
        .unwrap();
    api.core
        .blobs
        .put(
            &caravel_relays::media::blob_key(&relay.id, &blob.sha256),
            Bytes::from_static(b"bytes"),
        )
        .await
        .unwrap();

    let feed = api
        .request(
            "GET",
            &format!("/moderation?since={now}&until={now}"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(feed.status, StatusCode::OK);
    let items = feed.body["data"]["items"].as_array().unwrap();
    assert!(
        items
            .iter()
            .any(|item| item["type"] == "upload" && item["relay_id"] == relay.id)
    );

    let owners = api.bearer(&owner).await;
    let refused = api.request("GET", "/moderation", Some(&owners), None).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);

    let path = format!("/moderation/blob/{}/{}", relay.id, blob.sha256);
    let (status, headers, body) = api.raw("GET", &path, &admin, Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "image/png");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(&body[..], b"bytes");

    let missing = format!("/moderation/blob/{}/{}", relay.id, "b".repeat(64));
    assert_eq!(
        api.raw("GET", &missing, &admin, Vec::new()).await.0,
        StatusCode::NOT_FOUND
    );
    let malformed = format!("/moderation/blob/{}/not-a-hash", relay.id);
    assert_eq!(
        api.raw("GET", &malformed, &admin, Vec::new()).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}
