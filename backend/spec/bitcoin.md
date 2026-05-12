# `bitcoin` — Bitcoin / Lightning helpers

Small wrappers around the Bitcoin-facing services the app talks to: Nostr Wallet Connect (NWC) wallets for Lightning invoices/payments, and a fiat↔BTC spot price feed, plus the fiat-minor-units → millisatoshi conversion that ties them together. The billing-specific orchestration (which wallet pays which invoice, double-charge guards, DMs, etc.) lives in `spec/billing.md`.

## `pub struct Bitcoin`

Owns the app's Bitcoin-facing configuration: the system NWC wallet URL (the wallet used to *receive* payments — issue and look up bolt11 invoices) and the HTTP client used for the fiat↔BTC spot price feed.

Members:

- `system_nwc_url: String` - from `NWC_URL`
- `http: reqwest::Client`

### `pub fn from_env() -> Self`

Reads `NWC_URL` (the system / receiving wallet) and constructs a fresh `reqwest::Client`. Unlike the Stripe keys, `NWC_URL` is **optional**: if it's unset, Lightning operations fail at use time with a clear error rather than panicking at startup. This is what `Billing::new` calls.

### `pub fn system_wallet(&self) -> Result<Wallet>`

Returns the system `Wallet` (parsed from `system_nwc_url` with label `"system"`). Errors if `NWC_URL` is unset or malformed.

### `pub async fn fiat_minor_to_msats(&self, amount_due_minor: i64, currency: &str) -> Result<u64>`

Fetches the live BTC spot price (via `btc_spot_price_from_base` against Coinbase) for `currency` and converts `amount_due_minor` to millisatoshis via `fiat_minor_to_msats_from_quote`. `currency` is upper-cased before use.

## `pub struct Wallet`

A handle to a single NWC wallet. Each operation opens a fresh NWC connection and tears it down (`shutdown`) afterwards.

Member:

- `uri: NostrWalletConnectURI` - the parsed `nostr+walletconnect://…` URI

### `pub fn parse(uri: &str, label: &str) -> Result<Self>`

Parses an `nostr+walletconnect://` URI. `label` (e.g. `"system"` / `"tenant"`) only flavours the error message — `invalid {label} NWC URL` — so callers can tell which wallet was misconfigured.

### `pub async fn make_invoice(&self, amount_msats: u64, description: &str) -> Result<String>`

Issues a bolt11 invoice for `amount_msats` with the given description. Returns the bolt11 string.

### `pub async fn pay_invoice(&self, bolt11: String) -> Result<()>`

Pays a bolt11 invoice.

### `pub async fn invoice_settled(&self, bolt11: &str) -> Result<bool>`

Looks up a bolt11 invoice (previously issued by this wallet) and returns whether it has settled — true if the transaction state is `Settled` or `settled_at` is present.

## `pub async fn btc_spot_price_from_base(http: &reqwest::Client, api_base: &str, currency: &str) -> Result<f64>`

Fetches the BTC spot price denominated in `currency` (an ISO-4217 code) from a Coinbase-shaped API at `api_base` (`{api_base}/BTC-{currency}/spot`); production callers reach it via `Bitcoin::fiat_minor_to_msats` with the real Coinbase base, while tests can point it at a stub. Errors if the quote is missing, unparseable, or non-positive.

## `pub fn fiat_minor_to_msats_from_quote(amount_due_minor: i64, currency: &str, btc_price_in_fiat: f64) -> Result<u64>`

Converts a fiat amount expressed in minor units (cents, etc.) to millisatoshis, given a BTC price quote in that currency.

- Errors if `amount_due_minor <= 0` or `btc_price_in_fiat <= 0`
- Converts minor units to a major-unit amount using the currency's decimal exponent (most currencies 2; `JPY`/`KRW`/… 0; `BHD`/`KWD`/… 3 — following Stripe's currency conventions; an unrecognized or malformed code is an error)
- `msats = (amount_fiat / btc_price_in_fiat) * 100_000_000_000`
- Rounds **up** so we never under-charge, but snaps to the nearest integer when within `1e-6` of one to avoid floating-point artifacts at integer boundaries
- Errors if the result is non-finite, non-positive, or exceeds `u64::MAX`
