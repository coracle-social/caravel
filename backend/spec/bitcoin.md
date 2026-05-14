# `bitcoin` — fiat ↔ Bitcoin conversion

Free async helpers for pricing fiat amounts in Lightning units against a live BTC spot price. The NWC wallet lives in `spec/wallet.md`; billing orchestration lives in `spec/billing.md`.

## `pub async fn fiat_to_msats(amount_fiat_minor: i64, currency: &str) -> Result<u64>`

Converts a Stripe-style minor-unit fiat amount to millisatoshis using the live BTC spot price for `currency` and Stripe's per-currency decimal exponent (most currencies 2; `JPY`/`KRW`/… 0; `BHD`/`KWD`/… 3).

## `pub async fn get_bitcoin_price(currency: &str) -> Result<f64>`

Returns the current BTC spot price in `currency`, fetched from Coinbase's public spot-price endpoint.
