# `pub struct Wallet`

A handle to a single Nostr Wallet Connect (NWC) wallet. `Billing` holds one as its system wallet (receives — issues and looks up invoices); tenant wallets (pay invoices) are constructed ad-hoc from the decrypted `tenant.nwc_url` at the call site. Each operation opens a fresh NWC connection and shuts it down afterwards.

Member:

- `url: NostrWalletConnectURI` — the parsed `nostr+walletconnect://…` URI

## `pub fn from_url(url: &str) -> Result<Self>`

Parses an `nostr+walletconnect://` URI.

## `pub async fn make_invoice(&self, amount_msats: u64, description: &str, expiry_secs: u64) -> Result<String>`

Issues a bolt11 invoice for `amount_msats` with the given `description` and expiry, and returns the bolt11 string.

## `pub async fn pay_invoice(&self, bolt11: String) -> Result<()>`

Pays a bolt11 invoice.

## `pub async fn is_settled(&self, bolt11: &str) -> Result<bool>`

Returns whether a bolt11 invoice (previously issued by this wallet) has settled.
