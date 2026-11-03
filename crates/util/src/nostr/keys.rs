//! The workspace's general-purpose Nostr key: the platform's identity, a relay's,
//! and the key the platform encrypts its stored secrets to itself with.

use anyhow::{Result, anyhow};
use coracle_lib::events::{Event, EventContent};
use coracle_lib::keys::{PublicKey, SecretKey};
use coracle_lib::tags::{Tag, Tags};

/// A Nostr key: the platform's identity, a relay's, or anyone's. It encrypts to
/// itself with NIP-44 and signs the events its holder publishes.
#[derive(Clone)]
pub struct Key {
    secret: SecretKey,
    pubkey: PublicKey,
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Key")
            .field("pubkey", &self.pubkey.to_hex())
            .finish()
    }
}

impl Key {
    pub fn from_hex(secret: &str) -> Result<Self> {
        let secret = SecretKey::from_hex(secret).map_err(|e| anyhow!("invalid secret key: {e}"))?;
        let pubkey = secret.public_key();
        Ok(Self { secret, pubkey })
    }

    pub fn pubkey(&self) -> String {
        self.pubkey.to_hex()
    }

    pub fn secret_hex(&self) -> String {
        self.secret.to_hex()
    }

    /// NIP-44 encrypt `plaintext` to this key's holder.
    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        self.secret
            .nip44_encrypt(&self.pubkey, plaintext)
            .map_err(|e| anyhow!("encryption failed: {e}"))
    }

    pub fn decrypt(&self, ciphertext: &str) -> Result<String> {
        self.secret
            .nip44_decrypt(&self.pubkey, ciphertext)
            .map_err(|e| anyhow!("decryption failed: {e}"))
    }

    /// A signed event from this key.
    pub fn sign(&self, kind: u16, tags: Vec<Vec<String>>, content: &str, created_at: i64) -> Event {
        let tags: Tags = tags.into_iter().map(Tag).collect();
        let hashed = EventContent::new()
            .with_content(content)
            .with_tags(tags)
            .with_kind(kind)
            .with_created_at(created_at)
            .with_pubkey(self.secret.public_key())
            .with_id();
        let sig = self.secret.sign(hashed.id.as_bytes());
        hashed.with_sig(sig)
    }
}

/// A pubkey as the platform's rows hold it, or an internal error if a row holds
/// something that isn't one.
pub fn parse_pubkey(hex: &str) -> crate::error::Result<PublicKey> {
    PublicKey::from_hex(hex).map_err(|error| crate::error::Error::Internal(error.into()))
}

/// A fresh keypair as hex `(pubkey, secret)`.
pub fn generate_keypair() -> (String, String) {
    let secret = SecretKey::generate();
    (secret.public_key().to_hex(), secret.to_hex())
}
