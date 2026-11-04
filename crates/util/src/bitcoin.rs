//! The Bitcoin price, for pricing Lightning invoices in dollars.

use std::time::Duration;

use anyhow::{Result, anyhow};

/// Convert US cents to millisatoshis at the current spot price.
pub async fn usd_cents_to_msats(cents: i64) -> Result<u64> {
    let price = get_usd_price().await?;
    let usd = cents as f64 / 100.0;
    Ok((usd / price * 100_000_000_000.0).round() as u64)
}

#[derive(serde::Deserialize)]
struct CoinbaseSpotPriceResponse {
    data: CoinbaseSpotPriceData,
}

#[derive(serde::Deserialize)]
struct CoinbaseSpotPriceData {
    amount: String,
}

/// The current Bitcoin spot price in US dollars, from Coinbase.
async fn get_usd_price() -> Result<f64> {
    let resp = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?
        .get("https://api.coinbase.com/v2/prices/BTC-USD/spot")
        .send()
        .await?;
    let body: CoinbaseSpotPriceResponse = resp.error_for_status()?.json().await?;

    body.data
        .amount
        .parse::<f64>()
        .ok()
        .filter(|price| price.is_finite() && *price > 0.0)
        .ok_or_else(|| anyhow!("invalid BTC spot quote: {}", body.data.amount))
}
