//! Nostr Wallet Connect (NIP-47): asking a wallet over its relays to make an
//! invoice, pay one, or say whether one is settled. Requests and responses are
//! NIP-04 encrypted between the connection's secret and the wallet's key.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use aes::Aes256;
use anyhow::{Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use coracle_lib::encryption::shared_secret;
use coracle_lib::events::{Event, EventContent};
use coracle_lib::filters::Filter;
use coracle_lib::keys::{PublicKey, SecretKey};
use coracle_lib::tags::Tags;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::nostr::Client;

/// How long to wait for the wallet to answer.
const TIMEOUT: Duration = Duration::from_secs(60);

const REQUEST: u16 = 23194;
const RESPONSE: u16 = 23195;

/// A wallet reached through a `nostr+walletconnect://` url: the platform's own,
/// which issues invoices, or a tenant's, which pays them.
#[derive(Clone)]
pub struct Wallet {
    wallet: PublicKey,
    relays: Vec<String>,
    secret: SecretKey,
    client: Client,
}

/// A wallet's answer: the result, or why there is none.
#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<ResponseError>,
}

#[derive(Deserialize)]
struct ResponseError {
    code: String,
    message: String,
}

impl Wallet {
    pub fn from_url(url: &str) -> Result<Self> {
        let parsed = url::Url::parse(url)?;
        if parsed.scheme() != "nostr+walletconnect" {
            bail!("not a nostr+walletconnect url");
        }
        let wallet = PublicKey::from_hex(parsed.host_str().unwrap_or_default())?;

        let mut relays = Vec::new();
        let mut secret = None;
        for (key, value) in parsed.query_pairs() {
            match key.as_ref() {
                "relay" => relays.push(value.to_string()),
                "secret" => secret = Some(SecretKey::from_hex(&value)?),
                _ => {}
            }
        }
        let secret = secret.ok_or_else(|| anyhow!("the url has no secret"))?;
        if relays.is_empty() {
            bail!("the url has no relay");
        }

        Ok(Self {
            wallet,
            relays,
            client: Client::new(secret.clone()),
            secret,
        })
    }

    /// A Lightning invoice for `amount_msats`, payable for `expiry_secs`.
    pub async fn make_invoice(
        &self,
        amount_msats: u64,
        description: &str,
        expiry_secs: u64,
        now: i64,
    ) -> Result<String> {
        let params = json!({
            "amount": amount_msats,
            "description": description,
            "expiry": expiry_secs,
        });
        let result = self
            .call("make_invoice", params, now)
            .await
            .map_err(|error| anyhow!("failed to create invoice: {error}"))?;
        result["invoice"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("the wallet's answer has no invoice"))
    }

    pub async fn pay_invoice(&self, bolt11: &str, now: i64) -> Result<()> {
        self.call("pay_invoice", json!({ "invoice": bolt11 }), now)
            .await
            .map(drop)
    }

    /// Whether an invoice this wallet issued has been paid.
    pub async fn is_settled(&self, bolt11: &str, now: i64) -> Result<bool> {
        let result = self
            .call("lookup_invoice", json!({ "invoice": bolt11 }), now)
            .await
            .map_err(|error| anyhow!("failed to lookup invoice: {error}"))?;
        Ok(result["state"] == "settled" || result["settled_at"].as_i64().is_some())
    }

    /// Ask the wallet to run `method`, returning its result.
    async fn call(&self, method: &str, params: Value, now: i64) -> Result<Value> {
        let content = nip04_encrypt(
            &self.secret,
            &self.wallet,
            &json!({ "method": method, "params": params }).to_string(),
        );
        let hashed = EventContent::new()
            .with_content(content)
            .with_tags(Tags::new().add("p", [self.wallet.to_hex()]))
            .with_kind(REQUEST)
            .with_created_at(now)
            .with_pubkey(self.secret.public_key())
            .with_id();
        let sig = self.secret.sign(&hashed.id.to_bytes());
        let request: Event = hashed.with_sig(sig);

        let reply = Filter {
            authors: Some(BTreeSet::from([self.wallet])),
            kinds: Some(BTreeSet::from([RESPONSE])),
            tags: BTreeMap::from([("#e".to_string(), BTreeSet::from([request.id.to_hex()]))]),
            ..Default::default()
        };
        let answer = self
            .client
            .request(&self.relays, &request, &reply, now, TIMEOUT)
            .await?;

        let plaintext = nip04_decrypt(&self.secret, &self.wallet, &answer.content)?;
        let response: Response = serde_json::from_str(&plaintext)?;
        if let Some(error) = response.error {
            bail!("{}: {}", error.code, error.message);
        }
        response
            .result
            .ok_or_else(|| anyhow!("the wallet answered without a result"))
    }
}

type Encryptor = cbc::Encryptor<Aes256>;
type Decryptor = cbc::Decryptor<Aes256>;

/// NIP-04: AES-256-CBC under the raw ECDH secret, as `<base64>?iv=<base64>`.
fn nip04_encrypt(secret: &SecretKey, peer: &PublicKey, plaintext: &str) -> String {
    let key = shared_secret(secret, peer);
    let iv: [u8; 16] = rand::random();
    let ciphertext = Encryptor::new(&key.into(), &iv.into())
        .encrypt_padded_vec_mut::<Pkcs7>(plaintext.as_bytes());
    format!("{}?iv={}", BASE64.encode(ciphertext), BASE64.encode(iv))
}

fn nip04_decrypt(secret: &SecretKey, peer: &PublicKey, payload: &str) -> Result<String> {
    let (ciphertext, iv) = payload
        .split_once("?iv=")
        .ok_or_else(|| anyhow!("not a NIP-04 payload"))?;
    let ciphertext = BASE64.decode(ciphertext)?;
    let iv: [u8; 16] = BASE64
        .decode(iv)?
        .try_into()
        .map_err(|_| anyhow!("a NIP-04 iv is 16 bytes"))?;
    let key = shared_secret(secret, peer);
    let plaintext = Decryptor::new(&key.into(), &iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(&ciphertext)
        .map_err(|_| anyhow!("the payload doesn't decrypt"))?;
    Ok(String::from_utf8(plaintext)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nip04_round_trips_between_two_keys() {
        let alice = SecretKey::generate();
        let bob = SecretKey::generate();
        let payload = nip04_encrypt(&alice, &bob.public_key(), "pay me");
        assert_eq!(
            nip04_decrypt(&bob, &alice.public_key(), &payload).unwrap(),
            "pay me"
        );
    }

    #[test]
    fn a_wallet_url_names_its_wallet_relays_and_secret() {
        let wallet = SecretKey::generate().public_key().to_hex();
        let secret = SecretKey::generate().to_hex();
        let url = format!(
            "nostr+walletconnect://{wallet}?relay=wss%3A%2F%2Frelay.example&relay=wss%3A%2F%2Fother.example&secret={secret}&lud16=a%40b.c"
        );
        let parsed = Wallet::from_url(&url).unwrap();
        assert_eq!(parsed.wallet.to_hex(), wallet);
        assert_eq!(
            parsed.relays,
            ["wss://relay.example", "wss://other.example"]
        );
        assert!(Wallet::from_url("https://example.com").is_err());
    }
}
