use axum::{Json, Router, routing::get};
use backend::billing::{fetch_btc_spot_price_from_base, fiat_minor_to_msats_from_quote};

#[tokio::test]
async fn quote_endpoint_can_be_stubbed_deterministically() {
    async fn spot() -> Json<serde_json::Value> {
        Json(serde_json::json!({ "data": { "amount": "50000.00" } }))
    }

    let app = Router::new().route("/v2/prices/BTC-USD/spot", get(spot));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let addr = listener.local_addr().expect("get local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve quote stub");
    });

    let client = reqwest::Client::new();
    let base = format!("http://{addr}/v2/prices");
    let btc_price = fetch_btc_spot_price_from_base(&client, &base, "USD")
        .await
        .expect("fetch stubbed quote");

    assert_eq!(btc_price, 50_000.0);

    let msats =
        fiat_minor_to_msats_from_quote(100, "USD", btc_price).expect("convert quoted fiat amount");
    assert_eq!(msats, 2_000_000);
}
