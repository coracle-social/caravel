//! The platform's key, which is the robot's identity and the key the platform
//! encrypts its stored secrets to itself with.

use anyhow::{Result, anyhow};
use coracle_lib::events::{Event, EventContent};
use coracle_lib::keys::{PublicKey, SecretKey};
use coracle_lib::tags::{Tag, Tags};
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct PlatformKey {
    secret: SecretKey,
    pubkey: PublicKey,
}

impl std::fmt::Debug for PlatformKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformKey")
            .field("pubkey", &self.pubkey.to_hex())
            .finish()
    }
}

impl PlatformKey {
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

    /// NIP-44 encrypt `plaintext` from the platform to itself.
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
}

/// A fresh keypair as hex `(pubkey, secret)`.
pub fn generate_keypair() -> (String, String) {
    let secret = SecretKey::generate();
    (secret.public_key().to_hex(), secret.to_hex())
}

/// An unguessable token, hex encoded.
pub fn random_token() -> String {
    hex::encode(rand::random::<[u8; 32]>())
}

pub fn sha256_hex(input: &[u8]) -> String {
    hex::encode(Sha256::digest(input))
}

/// A relay's own key, which signs the events the relay publishes about itself.
#[derive(Clone)]
pub struct RelayKey {
    secret: SecretKey,
}

impl RelayKey {
    /// The key a relay's stored secret decrypts to.
    pub fn decrypt(platform_key: &PlatformKey, encrypted_secret: &str) -> Result<Self> {
        let secret = SecretKey::from_hex(&platform_key.decrypt(encrypted_secret)?)
            .map_err(|e| anyhow!("invalid relay secret: {e}"))?;
        Ok(Self { secret })
    }

    pub fn pubkey(&self) -> String {
        self.secret.public_key().to_hex()
    }

    /// A signed event from the relay.
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
