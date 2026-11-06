//! How the API authenticates: sessions bought with one NIP-98 signature, and which
//! routes need one.

mod common;

use axum::http::StatusCode;
use caravel_core::testing;
use caravel_core::testing::{API_URL, LEGACY_API_URL};
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
        testing::http_auth(&user, API_URL, "POST", now),
        testing::http_auth(&user, &url, "GET", now),
        testing::http_auth(&user, &url, "POST", now - 120),
        testing::http_auth(&user, &url, "POST", now + 120),
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
async fn a_signature_is_no_substitute_for_a_session() {
    let api = TestApi::new().await;
    let user = SecretKey::generate();
    let now = api.core.clock.now();

    for auth in [
        testing::http_auth(&user, API_URL, "GET", now),
        testing::http_auth(&user, &format!("{API_URL}/identity"), "GET", now),
    ] {
        let response = api.request("GET", "/identity", Some(&auth), None).await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED);
        assert_eq!(response.body["error"], "authorization url mismatch");
    }
}

#[tokio::test]
async fn a_fresh_signature_over_the_legacy_url_stands_in_for_a_session() {
    let api = TestApi::new().await;
    let user = SecretKey::generate();
    let now = api.core.clock.now();

    // Those clients sign one GET over the bare url and send it with every request.
    let auth = testing::http_auth(&user, LEGACY_API_URL, "GET", now - 600);
    let identity = api.request("GET", "/identity", Some(&auth), None).await;
    assert_eq!(identity.status, StatusCode::OK);
    assert_eq!(identity.body["data"]["pubkey"], user.public_key().to_hex());
    let pubkey = api.create_tenant(&user).await;
    let relays = api
        .request(
            "GET",
            &format!("/tenants/{pubkey}/relays"),
            Some(&auth),
            None,
        )
        .await;
    assert_eq!(relays.status, StatusCode::OK);

    for (auth, error) in [
        (
            testing::http_auth(&user, LEGACY_API_URL, "GET", now - 3600),
            "authorization is stale",
        ),
        (
            testing::http_auth(&user, &format!("{LEGACY_API_URL}/identity"), "GET", now),
            "authorization url mismatch",
        ),
    ] {
        let response = api.request("GET", "/identity", Some(&auth), None).await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED);
        assert_eq!(response.body["error"], error);
    }
}

/// Every route the router serves is in a class: the public ones answer without
/// credentials, the rest refuse a request without them, and admin routes refuse a tenant.
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
    // Reached without credentials, and finding nothing.
    let public_missing = [
        ("GET", "/domains/check?domain=missing.test".to_string()),
        ("GET", format!("/relays/{id}/offer")),
        ("POST", format!("/relays/{id}/invites")),
        ("GET", format!("/invites/{id}")),
        ("POST", format!("/invites/{id}/reconcile")),
    ];
    let authed = [
        ("POST", "/session".to_string()),
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
        ("GET", format!("/tenants/{tenant_pubkey}/payouts")),
        ("POST", format!("/tenants/{tenant_pubkey}/payouts/onboard")),
        (
            "POST",
            format!("/tenants/{tenant_pubkey}/payouts/dashboard"),
        ),
        ("POST", "/relays".to_string()),
        ("GET", format!("/relays/{id}")),
        ("PUT", format!("/relays/{id}")),
        ("GET", format!("/relays/{id}/members")),
        ("POST", format!("/relays/{id}/management")),
        ("GET", format!("/relays/{id}/rooms")),
        ("POST", format!("/relays/{id}/rooms/moderate")),
        ("GET", format!("/relays/{id}/events")),
        ("PUT", format!("/relays/{id}/members/{tenant_pubkey}")),
        ("DELETE", format!("/relays/{id}/members/{tenant_pubkey}")),
        ("GET", format!("/relays/{id}/activity")),
        ("POST", format!("/relays/{id}/deactivate")),
        ("POST", format!("/relays/{id}/reactivate")),
        ("POST", format!("/relays/{id}/purge")),
        ("POST", format!("/relays/{id}/erase")),
        ("GET", format!("/relays/{id}/export")),
        ("POST", format!("/relays/{id}/import")),
        ("GET", format!("/relays/{id}/memberships")),
        ("GET", format!("/relays/{id}/membership")),
        ("POST", format!("/relays/{id}/membership/checkout")),
        ("POST", format!("/relays/{id}/membership/reconcile")),
        ("POST", format!("/relays/{id}/membership/cancel")),
        ("GET", "/account".to_string()),
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
        ("POST", format!("/tenants/{tenant_pubkey}/delete")),
        ("DELETE", format!("/sessions/{tenant_pubkey}")),
        ("GET", "/relays".to_string()),
        ("GET", "/invoices".to_string()),
        ("GET", "/moderation".to_string()),
        ("GET", format!("/moderation/blob/{id}/{sha}")),
    ];

    let classified: Vec<&(&str, String)> = public
        .iter()
        .chain(&public_missing)
        .chain(&authed)
        .chain(&admin)
        .collect();
    let served = served_routes(&api).await;
    for (method, route) in &served {
        assert!(
            classified
                .iter()
                .any(|(m, path)| m == method && is_request_for(route, path)),
            "{method} {route} has no auth class"
        );
    }
    for (method, path) in &classified {
        assert!(
            served
                .iter()
                .any(|(m, route)| m == method && is_request_for(route, path)),
            "{method} {path} is not a route"
        );
    }

    for (method, path) in &public {
        let response = api.request(method, path, None, None).await;
        assert_eq!(response.status, StatusCode::OK, "{method} {path}");
    }

    for (method, path) in &public_missing {
        let body = serde_json::json!({ "checkout": "cs_missing" });
        let response = api.request(method, path, None, Some(body)).await;
        assert_eq!(response.status, StatusCode::NOT_FOUND, "{method} {path}");
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

/// Each method and route the router serves, read from the `Allow` header of a
/// method no route takes.
async fn served_routes(api: &TestApi) -> Vec<(String, &'static str)> {
    let mut served = Vec::new();
    for (route, _) in caravel_api::routes() {
        let path: Vec<&str> = route
            .split('/')
            .map(|segment| {
                if segment.starts_with('{') {
                    "x"
                } else {
                    segment
                }
            })
            .collect();
        let (status, headers, _) = api.raw("PROBE", &path.join("/"), "", Vec::new()).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{route}");
        let allow = headers["allow"].to_str().unwrap();
        for method in allow.split(',').filter(|method| *method != "HEAD") {
            served.push((method.to_string(), route));
        }
    }
    served
}

/// Whether `path`, ignoring its query, requests `route`, whose `{}` segments match any.
fn is_request_for(route: &str, path: &str) -> bool {
    let path: Vec<&str> = path.split('?').next().unwrap().split('/').collect();
    let route: Vec<&str> = route.split('/').collect();
    route.len() == path.len()
        && route
            .iter()
            .zip(&path)
            .all(|(route, path)| route.starts_with('{') || route == path)
}

#[tokio::test]
async fn banning_a_tenant_nobody_is_not_found() {
    let api = TestApi::new().await;
    let admin = api.bearer(&api.admin).await;
    let nobody = SecretKey::generate().public_key().to_hex();
    let banned = api
        .request(
            "POST",
            &format!("/tenants/{nobody}/ban"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(banned.status, StatusCode::NOT_FOUND);
}
