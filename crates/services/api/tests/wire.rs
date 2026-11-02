//! The shapes the API's clients rely on, Flotilla's hosting client among them:
//! field names, and flags as `0`/`1`.

mod common;

use axum::http::StatusCode;
use caravel_core::billing;
use caravel_core::billing::Collection;
use caravel_core::testing::RELAY_DOMAIN;
use common::*;
use coracle_lib::keys::SecretKey;
use serde_json::json;

const RELAY_FIELDS: [&str; 30] = [
    "blossom_authenticated_read",
    "blossom_enabled",
    "created_at",
    "custom_domain",
    "custom_domain_verified",
    "erased_at",
    "groups_enabled",
    "id",
    "info_description",
    "info_icon",
    "info_name",
    "livekit_enabled",
    "management_enabled",
    "member_interval",
    "member_price",
    "members_can_invite",
    "plan_id",
    "policy_anonymous_read",
    "policy_public_join",
    "policy_public_read",
    "policy_public_write",
    "policy_strip_signatures",
    "pubkey",
    "purged_at",
    "push_enabled",
    "redirect_from",
    "status",
    "subdomain",
    "tenant_pubkey",
    "zooid_domain",
];

#[tokio::test]
async fn a_relay_reads_back_in_the_shape_clients_expect() {
    let api = TestApi::new().await;
    let tenant = SecretKey::generate();
    let tenant_pubkey = api.create_tenant(&tenant).await;
    let bearer = api.bearer(&tenant).await;

    let created = api
        .request(
            "POST",
            "/relays",
            Some(&bearer),
            Some(json!({
                "tenant_pubkey": tenant_pubkey,
                "subdomain": "wire-shape",
                "zooid_domain": RELAY_DOMAIN,
                "plan_id": "basic",
                "info_name": "Wire",
                "policy_public_read": 1,
                "blossom_enabled": true,
            })),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    assert_eq!(created.body["code"], "ok");

    let relay = &created.body["data"];
    assert_eq!(keys(relay), RELAY_FIELDS);
    assert_eq!(relay["zooid_domain"], RELAY_DOMAIN);
    assert_eq!(relay["status"], "active");
    assert_eq!(relay["policy_public_read"], 1);
    assert_eq!(relay["policy_public_write"], 0);
    assert_eq!(relay["blossom_enabled"], 1);
    assert_eq!(relay["groups_enabled"], 1);
    assert_eq!(relay["redirect_from"], json!([]));
    assert_eq!(relay["purged_at"], json!(null));
    assert_eq!(relay["pubkey"].as_str().map(str::len), Some(64));

    let id = relay["id"].as_str().unwrap();
    let fetched = api
        .request("GET", &format!("/relays/{id}"), Some(&bearer), None)
        .await;
    assert_eq!(&fetched.body["data"], relay);

    let updated = api
        .request(
            "PUT",
            &format!("/relays/{id}"),
            Some(&bearer),
            Some(json!({ "push_enabled": 0 })),
        )
        .await;
    assert_eq!(updated.body["data"]["push_enabled"], 0);

    let activity = api
        .request(
            "GET",
            &format!("/relays/{id}/activity"),
            Some(&bearer),
            None,
        )
        .await;
    let entries = activity.body["data"]["activity"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["activity_type"], "update_relay");
    assert_eq!(
        keys(&entries[0]),
        [
            "activity_type",
            "billed_at",
            "created_at",
            "id",
            "relay_id",
            "snapshot",
            "tenant_pubkey"
        ]
    );

    let members = api
        .request("GET", &format!("/relays/{id}/members"), Some(&bearer), None)
        .await;
    assert_eq!(members.body["data"]["roles"], json!([]));
    let owner = &members.body["data"]["members"][0];
    assert_eq!(keys(owner), ["created_at", "pubkey", "roles", "source"]);
    assert_eq!(owner["pubkey"], tenant_pubkey);
    assert_eq!(owner["source"], "owner");
}

#[tokio::test]
async fn validation_failures_carry_a_code() {
    let api = TestApi::new().await;
    let tenant = SecretKey::generate();
    let tenant_pubkey = api.create_tenant(&tenant).await;
    let bearer = api.bearer(&tenant).await;

    let create = |subdomain: &str, domain: &str, plan: &str, blossom: i64| {
        json!({
            "tenant_pubkey": tenant_pubkey,
            "subdomain": subdomain,
            "zooid_domain": domain,
            "plan_id": plan,
            "blossom_enabled": blossom,
        })
    };
    let with_custom_domain = |subdomain: &str, custom_domain: &str| {
        let mut body = create(subdomain, RELAY_DOMAIN, "free", 0);
        body["custom_domain"] = json!(custom_domain);
        body
    };

    for (body, code) in [
        (create("a", RELAY_DOMAIN, "free", 0), "invalid-subdomain"),
        (
            create("admin", RELAY_DOMAIN, "free", 0),
            "invalid-subdomain",
        ),
        // Its canonical host would be the dashboards'.
        (create("app", RELAY_DOMAIN, "free", 0), "invalid-subdomain"),
        // The platform's own host can't serve a relay.
        (
            with_custom_domain("fine-name", "app.relay.test"),
            "reserved-domain",
        ),
        (
            create("fine-name", "elsewhere.test", "free", 0),
            "invalid-zooid-domain",
        ),
        (create("fine-name", RELAY_DOMAIN, "nope", 0), "invalid-plan"),
        (
            create("fine-name", RELAY_DOMAIN, "free", 1),
            "premium-feature",
        ),
    ] {
        let response = api
            .request("POST", "/relays", Some(&bearer), Some(body))
            .await;
        assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(response.body["code"], code);
    }

    let first = api
        .request(
            "POST",
            "/relays",
            Some(&bearer),
            Some(create("taken-name", RELAY_DOMAIN, "free", 0)),
        )
        .await;
    assert_eq!(first.status, StatusCode::CREATED);
    let second = api
        .request(
            "POST",
            "/relays",
            Some(&bearer),
            Some(create("taken-name", RELAY_DOMAIN, "free", 0)),
        )
        .await;
    assert_eq!(second.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(second.body["code"], "subdomain-exists");

    let id = first.body["data"]["id"].as_str().unwrap();
    let path = format!("/relays/{id}/reactivate");
    let reactivated = api.request("POST", &path, Some(&bearer), None).await;
    assert_eq!(reactivated.status, StatusCode::BAD_REQUEST);
    assert_eq!(reactivated.body["code"], "relay-is-active");

    // A custom domain is taken once another relay has verified it.
    let claimed = api
        .request(
            "POST",
            "/relays",
            Some(&bearer),
            Some(with_custom_domain("claimed-name", "taken.example")),
        )
        .await;
    assert_eq!(claimed.status, StatusCode::CREATED);
    let claimed_id = claimed.body["data"]["id"].as_str().unwrap();
    caravel_billing::relays::commands::set_custom_domain_verified(
        api.core.db.pool(),
        claimed_id,
        "taken.example",
        true,
    )
    .await
    .unwrap();
    let taken = api
        .request(
            "POST",
            "/relays",
            Some(&bearer),
            Some(with_custom_domain("another-name", "taken.example")),
        )
        .await;
    assert_eq!(taken.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(taken.body["code"], "domain-exists");
}

#[tokio::test]
async fn an_internal_failure_keeps_its_cause_to_the_server() {
    let api = TestApi::new().await;
    let bearer = api.bearer(&SecretKey::generate()).await;
    api.core.db.pool().close().await;

    let response = api.request("GET", "/identity", Some(&bearer), None).await;
    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response.body,
        json!({ "error": "internal error", "code": "internal" })
    );
}

#[tokio::test]
async fn a_tenant_and_its_invoices_read_back_in_the_shape_clients_expect() {
    let api = TestApi::new().await;
    let tenant = SecretKey::generate();
    let tenant_pubkey = api.create_tenant(&tenant).await;
    let bearer = api.bearer(&tenant).await;

    let fetched = api
        .request(
            "GET",
            &format!("/tenants/{tenant_pubkey}"),
            Some(&bearer),
            None,
        )
        .await;
    assert_eq!(
        keys(&fetched.body["data"]),
        [
            "banned_at",
            "billing_anchor",
            "churned_at",
            "created_at",
            "nwc_error",
            "nwc_is_set",
            "pubkey",
            "return_url",
            "stripe_customer_id",
            "stripe_error",
            "stripe_payment_method_id",
        ]
    );
    assert_eq!(fetched.body["data"]["nwc_is_set"], false);

    api.request(
        "POST",
        "/relays",
        Some(&bearer),
        Some(json!({
            "tenant_pubkey": tenant_pubkey,
            "subdomain": "billed-relay",
            "zooid_domain": RELAY_DOMAIN,
            "plan_id": "basic",
        })),
    )
    .await;

    let draft = api
        .request(
            "GET",
            &format!("/tenants/{tenant_pubkey}/invoices/draft"),
            Some(&bearer),
            None,
        )
        .await;
    assert_eq!(draft.body["data"], json!(null));

    let row = caravel_billing::tenants::queries::get_tenant(api.core.db.pool(), &tenant_pubkey)
        .await
        .unwrap()
        .unwrap();
    billing::reconcile_subscription(&api.core, &row, Collection::Settle)
        .await
        .unwrap();

    let invoices = api
        .request(
            "GET",
            &format!("/tenants/{tenant_pubkey}/invoices"),
            Some(&bearer),
            None,
        )
        .await;
    let invoice = &invoices.body["data"][0];
    assert_eq!(
        keys(invoice),
        [
            "amount",
            "created_at",
            "id",
            "method",
            "notified_at",
            "paid_at",
            "period_end",
            "period_start",
            "tenant_pubkey",
            "voided_at",
        ]
    );
    assert_eq!(invoice["amount"], 500);

    let id = invoice["id"].as_str().unwrap();
    let items = api
        .request("GET", &format!("/invoices/{id}/items"), Some(&bearer), None)
        .await;
    assert_eq!(
        keys(&items.body["data"][0]),
        [
            "activity_id",
            "amount",
            "created_at",
            "description",
            "id",
            "invoice_id",
            "plan_id",
            "relay_id",
            "tenant_pubkey",
            "voided_at",
        ]
    );

    let stranger = SecretKey::generate();
    let stranger_bearer = api.bearer(&stranger).await;
    let forbidden = api
        .request(
            "GET",
            &format!("/invoices/{id}"),
            Some(&stranger_bearer),
            None,
        )
        .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn plans_read_back_in_the_shape_clients_expect() {
    let api = TestApi::new().await;
    let plans = api.request("GET", "/plans", None, None).await;

    assert_eq!(
        keys(&plans.body["data"][0]),
        [
            "amount",
            "av_hours",
            "av_overage",
            "blossom",
            "id",
            "livekit",
            "members",
            "name",
            "storage_gb",
            "storage_overage",
        ]
    );
}
