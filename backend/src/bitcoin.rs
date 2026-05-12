//! Small wrappers around the Bitcoin-facing services this app talks to: Nostr
//! Wallet Connect wallets (for Lightning invoices/payments) and a fiat↔BTC spot
//! price feed, plus the fiat-minor-units → millisatoshi conversion that ties them
//! together. The billing-specific orchestration lives in [`crate::billing`].

use anyhow::{Result, anyhow};
use nwc::prelude::{
    LookupInvoiceRequest, LookupInvoiceResponse, MakeInvoiceRequest, NWC, NostrWalletConnectURI,
    PayInvoiceRequest, TransactionState,
};

const COINBASE_SPOT_API: &str = "https://api.coinbase.com/v2/prices";
/// Millisatoshis per bitcoin.
const MSATS_PER_BTC: f64 = 100_000_000_000.0;

/// Owns the app's Bitcoin-facing configuration: the system NWC wallet URL (the
/// wallet used to *receive* payments — issue and look up bolt11 invoices) and the
/// HTTP client used for the fiat↔BTC spot price feed.
#[derive(Clone)]
pub struct Bitcoin {
    system_nwc_url: String,
    http: reqwest::Client,
}

impl Bitcoin {
    /// Reads `NWC_URL` (the system / receiving wallet). Unlike the Stripe keys this
    /// is optional: if it's unset, Lightning operations fail at use time with a
    /// clear error rather than at startup.
    pub fn from_env() -> Self {
        Self {
            system_nwc_url: std::env::var("NWC_URL").unwrap_or_default(),
            http: reqwest::Client::new(),
        }
    }

    /// The system wallet — issues and looks up the bolt11 invoices we want paid to
    /// us. Errors if `NWC_URL` is unset or malformed.
    pub fn system_wallet(&self) -> Result<Wallet> {
        Wallet::parse(&self.system_nwc_url, "system")
    }

    /// Fetches the live BTC spot price and converts a fiat amount in minor units
    /// (cents, etc.) to millisatoshis.
    pub async fn fiat_minor_to_msats(&self, amount_due_minor: i64, currency: &str) -> Result<u64> {
        let currency = currency.to_uppercase();
        let btc_price = btc_spot_price_from_base(&self.http, COINBASE_SPOT_API, &currency).await?;
        fiat_minor_to_msats_from_quote(amount_due_minor, &currency, btc_price)
    }
}

#[derive(serde::Deserialize)]
struct CoinbaseSpotPriceResponse {
    data: CoinbaseSpotPriceData,
}

#[derive(serde::Deserialize)]
struct CoinbaseSpotPriceData {
    amount: String,
}

/// A handle to a single Nostr Wallet Connect wallet. Each operation opens a fresh
/// connection and tears it down afterwards.
pub struct Wallet {
    uri: NostrWalletConnectURI,
}

impl Wallet {
    /// Parses an `nostr+walletconnect://` URI. `label` only flavours the error
    /// message so callers can tell which wallet was misconfigured.
    pub fn parse(uri: &str, label: &str) -> Result<Self> {
        let uri = uri
            .parse::<NostrWalletConnectURI>()
            .map_err(|_| anyhow!("invalid {label} NWC URL"))?;
        Ok(Self { uri })
    }

    /// Issues a bolt11 invoice for `amount_msats`.
    pub async fn make_invoice(&self, amount_msats: u64, description: &str) -> Result<String> {
        let nwc = NWC::new(self.uri.clone());
        let result = nwc
            .make_invoice(MakeInvoiceRequest {
                amount: amount_msats,
                description: Some(description.to_string()),
                description_hash: None,
                expiry: None,
            })
            .await;
        nwc.shutdown().await;
        Ok(result
            .map_err(|e| anyhow!("failed to create invoice: {e}"))?
            .invoice)
    }

    /// Pays a bolt11 invoice.
    pub async fn pay_invoice(&self, bolt11: String) -> Result<()> {
        let nwc = NWC::new(self.uri.clone());
        let result = nwc.pay_invoice(PayInvoiceRequest::new(bolt11)).await;
        nwc.shutdown().await;
        result.map(|_| ()).map_err(|e| anyhow!("{e}"))
    }

    /// Returns whether a bolt11 invoice (previously issued by this wallet) has been
    /// settled.
    pub async fn invoice_settled(&self, bolt11: &str) -> Result<bool> {
        let nwc = NWC::new(self.uri.clone());
        let result = nwc
            .lookup_invoice(LookupInvoiceRequest {
                payment_hash: None,
                invoice: Some(bolt11.to_string()),
            })
            .await;
        nwc.shutdown().await;
        let response = result.map_err(|e| anyhow!("failed to lookup invoice: {e}"))?;
        Ok(lookup_invoice_response_is_settled(&response))
    }
}

fn lookup_invoice_response_is_settled(response: &LookupInvoiceResponse) -> bool {
    response.state == Some(TransactionState::Settled) || response.settled_at.is_some()
}

/// Fetches the BTC spot price denominated in `currency` (an ISO-4217 code) from a
/// Coinbase-shaped API at `api_base`. Exposed so tests can stub the price feed;
/// production callers go through [`Bitcoin::fiat_minor_to_msats`].
pub async fn btc_spot_price_from_base(
    http: &reqwest::Client,
    api_base: &str,
    currency: &str,
) -> Result<f64> {
    let pair = format!("BTC-{currency}");
    let url = format!("{}/{pair}/spot", api_base.trim_end_matches('/'));
    let resp = http.get(url).send().await?;
    let body: CoinbaseSpotPriceResponse = resp.error_for_status()?.json().await?;

    let amount = body
        .data
        .amount
        .parse::<f64>()
        .map_err(|e| anyhow!("invalid BTC spot quote for {currency}: {e}"))?;
    if amount <= 0.0 {
        return Err(anyhow!(
            "invalid non-positive BTC spot quote for {currency}"
        ));
    }
    Ok(amount)
}

/// Converts a fiat amount expressed in minor units (cents, etc.) to millisatoshis,
/// given a BTC price quote in that currency. Rounds up so we never under-charge,
/// but snaps to the nearest integer when within a hair of one to avoid floating
/// point artifacts at integer boundaries.
pub fn fiat_minor_to_msats_from_quote(
    amount_due_minor: i64,
    currency: &str,
    btc_price_in_fiat: f64,
) -> Result<u64> {
    if amount_due_minor <= 0 {
        return Err(anyhow!("amount_due must be positive"));
    }
    if btc_price_in_fiat <= 0.0 {
        return Err(anyhow!("btc_price_in_fiat must be positive"));
    }

    let divisor = 10_f64.powi(currency_minor_exponent(currency)? as i32);
    let amount_fiat = (amount_due_minor as f64) / divisor;
    let amount_btc = amount_fiat / btc_price_in_fiat;
    let raw_msats = amount_btc * MSATS_PER_BTC;
    let amount_msats = if (raw_msats - raw_msats.round()).abs() < 1e-6 {
        raw_msats.round()
    } else {
        raw_msats.ceil()
    };

    if !amount_msats.is_finite() || amount_msats <= 0.0 || amount_msats > u64::MAX as f64 {
        return Err(anyhow!("calculated msat amount is out of bounds"));
    }
    Ok(amount_msats as u64)
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

#[cfg(test)]
mod tests {
    use super::fiat_minor_to_msats_from_quote;

    #[test]
    fn converts_usd_minor_units_with_quote() {
        let msats = fiat_minor_to_msats_from_quote(100, "usd", 100_000.0)
            .expect("conversion should succeed");
        assert_eq!(msats, 1_000_000);
    }

    #[test]
    fn converts_zero_decimal_currency_with_quote() {
        let msats = fiat_minor_to_msats_from_quote(100, "jpy", 10_000_000.0)
            .expect("conversion should succeed");
        assert_eq!(msats, 1_000_000);
    }

    #[test]
    fn rejects_malformed_currency_code() {
        // Not three ASCII letters: rejected outright.
        assert!(fiat_minor_to_msats_from_quote(100, "usdd", 100_000.0).is_err());
        assert!(fiat_minor_to_msats_from_quote(100, "us1", 100_000.0).is_err());
    }
}
