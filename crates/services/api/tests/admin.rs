//! The admins' lists of every tenant, relay and invoice on the platform, a page
//! at a time.

mod common;

use axum::http::StatusCode;
use caravel_billing::relays::{PLAN_FREE, RelaySettings};
use caravel_billing::tenants::{BillingPeriod, InvoiceItem};
use caravel_core::hosting;
use caravel_core::testing;
use caravel_util::error::Error;
use common::*;
use coracle_lib::keys::SecretKey;

/// The start of these tests' platform, so nothing another test left behind is
/// newer.
const NOW: i64 = 4_102_444_800;

#[tokio::test]
async fn an_admin_list_pages_back_from_the_newest() {
    let api = TestApi::new().await;
    api.core.clock.pin(NOW);
    let bearer = api.bearer(&api.admin).await;

    let (mut tenants, mut relays, mut invoices) = (Vec::new(), Vec::new(), Vec::new());
    for n in 0..3 {
        api.core.clock.pin(NOW + n);
        let tenant = api.create_tenant(&SecretKey::generate()).await;
        let relay = hosting::create_relay(
            &api.core,
            &tenant,
            RelaySettings {
                subdomain: Some(testing::subdomain()),
                domain: Some(testing::RELAY_DOMAIN.to_string()),
                plan_id: Some(PLAN_FREE.to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("failed to create a relay");

        invoices.push(cut_invoice(&api, &tenant, &relay.id, NOW + n).await);
        tenants.push(tenant);
        relays.push(relay.id);
    }
    tenants.reverse();
    relays.reverse();
    invoices.reverse();

    for (path, key, created) in [
        ("/tenants", "pubkey", &tenants),
        ("/relays", "id", &relays),
        ("/invoices", "id", &invoices),
    ] {
        let first = page(&api, &bearer, &format!("{path}?limit=2"), key).await;
        assert_eq!(first, created[..2], "{path}");

        let rest = page(
            &api,
            &bearer,
            &format!("{path}?limit=2&before={}", first[1]),
            key,
        )
        .await;
        assert_eq!(rest, created[2..], "{path}");
    }
}

/// A page of an admin list, by each item's `key`, newest first.
async fn page(api: &TestApi, bearer: &str, path: &str, key: &str) -> Vec<String> {
    let response = api.request("GET", path, Some(bearer), None).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.body["data"]
        .as_array()
        .expect("not a list")
        .iter()
        .map(|item| item[key].as_str().expect("no id").to_string())
        .collect()
}

/// Cut an invoice for `tenant` at `at`, from an item worth cutting one for.
async fn cut_invoice(api: &TestApi, tenant: &str, relay: &str, at: i64) -> String {
    api.core
        .db
        .with_tx(async |conn| {
            caravel_billing::tenants::commands::billing::insert_invoice_item(
                &mut *conn,
                &InvoiceItem {
                    id: format!("item-{at}"),
                    invoice_id: None,
                    activity_id: None,
                    tenant_pubkey: tenant.to_string(),
                    relay_id: relay.to_string(),
                    plan_id: PLAN_FREE.to_string(),
                    amount: 500,
                    description: "A membership".to_string(),
                    created_at: at,
                    voided_at: None,
                },
            )
            .await?;
            let invoice =
                caravel_billing::tenants::commands::billing::insert_invoice_for_outstanding_items(
                    conn,
                    tenant,
                    &BillingPeriod {
                        start: at,
                        end: at + 1,
                    },
                    false,
                    at,
                )
                .await?
                .ok_or_else(|| Error::internal("no invoice was cut"))?;
            Ok::<_, Error>(invoice.id)
        })
        .await
        .expect("failed to cut an invoice")
}
