//! How the API authenticates: sessions bought with one NIP-98 signature, the
//! reusable base-url signatures older clients send, and which routes need either.

mod common;

use axum::http::StatusCode;
use caravel_core::testing::API_URL;
use common::*;
use coracle_lib::keys::SecretKey;

#[tokio::test]
async fn a_signature_buys_a_session_until_it_is_deleted() {
    let api = TestApi::new().await;
    let user = SecretKey::generate();

    let bearer = api.bearer(&user).await;
    let identity = api.request("GET", "/identity", Some(&bearer), None).await;
    assert_eq!(identity.status, StatusCode::OK);
    assert_eq!(identity.body["data"]["pubkey"], user.public_key().to_hex());
    assert_eq!(identity.body["data"]["is_admin"], false);

    let deleted = api.request("DELETE", "/session", Some(&bearer), None).await;
    assert_eq!(deleted.status, StatusCode::OK);

    let after = api.request("GET", "/identity", Some(&bearer), None).await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_session_signature_must_be_for_this_request_and_fresh() {
    let api = TestApi::new().await;
    let user = SecretKey::generate();
    let now = api.core.clock.now();
    let url = format!("{API_URL}/session");

    for auth in [
        sign_http_auth(&user, API_URL, "POST", now),
        sign_http_auth(&user, &url, "GET", now),
        sign_http_auth(&user, &url, "POST", now - 120),
        sign_http_auth(&user, &url, "POST", now + 120),
    ] {
        let response = api.request("POST", "/session", Some(&auth), None).await;
        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{:?}",
            response.body
        );
    }
}

#[tokio::test]
async fn a_session_expires_after_thirty_days() {
    let api = TestApi::new().await;
    let user = SecretKey::generate();
    let start = api.core.clock.now();
    api.core.clock.pin(start);

    let bearer = api.bearer(&user).await;
    api.core.clock.pin(start + 30 * 24 * 60 * 60 - 1);
    assert_eq!(
        api.request("GET", "/identity", Some(&bearer), None)
            .await
            .status,
        StatusCode::OK
    );

    api.core.clock.pin(start + 30 * 24 * 60 * 60);
    assert_eq!(
        api.request("GET", "/identity", Some(&bearer), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn an_admin_can_revoke_every_session_a_pubkey_holds() {
    let api = TestApi::new().await;
    let user = SecretKey::generate();
    let first = api.bearer(&user).await;
    let second = api.bearer(&user).await;
    let admin = api.bearer(&api.admin).await;

    let path = format!("/sessions/{}", user.public_key().to_hex());
    assert_eq!(
        api.request("DELETE", &path, Some(&first), None)
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    let revoked = api.request("DELETE", &path, Some(&admin), None).await;
    assert_eq!(revoked.body["data"]["revoked"], 2);
    for bearer in [first, second] {
        assert_eq!(
            api.request("GET", "/identity", Some(&bearer), None)
                .await
                .status,
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn a_base_url_signature_works_for_fifteen_minutes() {
    let api = TestApi::new().await;
    let user = SecretKey::generate();
    let now = api.core.clock.now();

    let fresh = sign_http_auth(&user, API_URL, "GET", now - 14 * 60);
    assert_eq!(
        api.request("GET", "/identity", Some(&fresh), None)
            .await
            .status,
        StatusCode::OK
    );

    let stale = sign_http_auth(&user, API_URL, "GET", now - 16 * 60);
    assert_eq!(
        api.request("GET", "/identity", Some(&stale), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );

    let elsewhere = sign_http_auth(&user, "http://elsewhere.test", "GET", now);
    assert_eq!(
        api.request("GET", "/identity", Some(&elsewhere), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

/// Every route but the public ones refuses a request without credentials, and the
/// admin routes refuse a tenant.
#[tokio::test]
async fn every_route_has_an_auth_class() {
    let api = TestApi::new().await;
    let tenant = SecretKey::generate();
    let tenant_pubkey = api.create_tenant(&tenant).await;
    let bearer = api.bearer(&tenant).await;
    let id = "missing";
    let sha = "0".repeat(64);

    let public = [
        ("GET", "/plans".to_string()),
        ("GET", "/plans/free".to_string()),
    ];
    let authed = [
        ("GET", "/identity".to_string()),
        ("POST", "/tenants".to_string()),
        ("GET", format!("/tenants/{tenant_pubkey}")),
        ("PUT", format!("/tenants/{tenant_pubkey}")),
        ("GET", format!("/tenants/{tenant_pubkey}/relays")),
        ("GET", format!("/tenants/{tenant_pubkey}/invoices")),
        ("GET", format!("/tenants/{tenant_pubkey}/invoices/draft")),
        (
            "GET",
            format!("/tenants/{tenant_pubkey}/invoices/draft/items"),
        ),
        ("POST", format!("/tenants/{tenant_pubkey}/reconcile")),
        ("GET", format!("/tenants/{tenant_pubkey}/stripe/session")),
        ("POST", "/relays".to_string()),
        ("GET", format!("/relays/{id}")),
        ("PUT", format!("/relays/{id}")),
        ("GET", format!("/relays/{id}/members")),
        ("PUT", format!("/relays/{id}/members/{tenant_pubkey}")),
        ("DELETE", format!("/relays/{id}/members/{tenant_pubkey}")),
        ("GET", format!("/relays/{id}/activity")),
        ("POST", format!("/relays/{id}/deactivate")),
        ("POST", format!("/relays/{id}/reactivate")),
        ("POST", format!("/relays/{id}/purge")),
        ("GET", format!("/relays/{id}/export")),
        ("POST", format!("/relays/{id}/import")),
        ("GET", format!("/invoices/{id}")),
        ("GET", format!("/invoices/{id}/items")),
        ("POST", format!("/invoices/{id}/reconcile")),
        ("POST", format!("/invoices/{id}/bolt11")),
        ("POST", format!("/invoices/{id}/checkout")),
        ("DELETE", "/session".to_string()),
    ];
    let admin = [
        ("GET", "/tenants".to_string()),
        ("POST", format!("/tenants/{tenant_pubkey}/ban")),
        ("POST", format!("/tenants/{tenant_pubkey}/unban")),
        ("DELETE", format!("/sessions/{tenant_pubkey}")),
        ("GET", "/relays".to_string()),
        ("GET", "/invoices".to_string()),
        ("GET", "/moderation".to_string()),
        ("GET", format!("/moderation/blob/{id}/{sha}")),
    ];

    for (method, path) in &public {
        let response = api.request(method, path, None, None).await;
        assert_eq!(response.status, StatusCode::OK, "{method} {path}");
    }

    for (method, path) in authed.iter().chain(&admin) {
        let response = api.request(method, path, None, None).await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{method} {path}");
    }

    for (method, path) in &admin {
        let response = api.request(method, path, Some(&bearer), None).await;
        assert_eq!(response.status, StatusCode::FORBIDDEN, "{method} {path}");
    }
}
