use anyhow::{Result, anyhow};

pub async fn fiat_to_msats(amount_fiat_minor: i64, currency: &str) -> Result<u64> {
    let price = get_bitcoin_price(&currency.to_uppercase()).await?;
    let divisor = 10_f64.powi(currency_minor_exponent(currency)? as i32);
    let amount_fiat = (amount_fiat_minor as f64) / divisor;
    let amount_msats = (amount_fiat / price * 100_000_000_000.0).round();
    Ok(amount_msats as u64)
}

#[derive(serde::Deserialize)]
struct CoinbaseSpotPriceResponse {
    data: CoinbaseSpotPriceData,
}

#[derive(serde::Deserialize)]
struct CoinbaseSpotPriceData {
    amount: String,
}

pub async fn get_bitcoin_price(currency: &str) -> Result<f64> {
    let http = reqwest::Client::new();
    let url = format!("https://api.coinbase.com/v2/prices/BTC-{currency}/spot");
    let resp = http.get(url).send().await?;
    let body: CoinbaseSpotPriceResponse = resp.error_for_status()?.json().await?;

    Ok(body
        .data
        .amount
        .parse::<f64>()
        .map_err(|e| anyhow!("invalid BTC spot quote for {currency}: {e}"))?)
}

/// Number of decimal places in `currency`'s minor unit, following Stripe's
/// currency conventions (most are 2, JPY/KRW/… are 0, BHD/KWD/… are 3).
fn currency_minor_exponent(currency: &str) -> Result<u8> {
    let normalized = currency.to_uppercase();
    let exponent = match normalized.as_str() {
        // Zero-decimal currencies in Stripe.
        "BIF" | "CLP" | "DJF" | "GNF" | "JPY" | "KMF" | "KRW" | "MGA" | "PYG" | "RWF" | "UGX"
        | "VND" | "VUV" | "XAF" | "XOF" | "XPF" => 0,
        // Three-decimal currencies in Stripe.
        "BHD" | "JOD" | "KWD" | "OMR" | "TND" => 3,
        _ if normalized.chars().all(|c| c.is_ascii_alphabetic()) && normalized.len() == 3 => 2,
        _ => return Err(anyhow!("invalid currency code: {currency}")),
    };
    Ok(exponent)
}
