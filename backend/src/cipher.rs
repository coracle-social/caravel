use anyhow::{Result, anyhow};
use nostr_sdk::prelude::*;

pub fn encrypt(plaintext: &str) -> Result<String> {
    let keys = load_key()?;
    nip44::encrypt(
        keys.secret_key(),
        &keys.public_key(),
        plaintext,
        nip44::Version::V2,
    )
    .map_err(|e| anyhow!("encryption failed: {e}"))
}

pub fn decrypt(ciphertext: &str) -> Result<String> {
    let keys = load_key()?;
    nip44::decrypt(keys.secret_key(), &keys.public_key(), ciphertext)
        .map_err(|e| anyhow!("decryption failed: {e}"))
}

fn load_key() -> Result<Keys> {
    let secret = std::env::var("ENCRYPTION_SECRET")
        .map_err(|_| anyhow!("missing ENCRYPTION_SECRET environment variable"))?;
    if secret.trim().is_empty() {
        return Err(anyhow!("ENCRYPTION_SECRET is empty"));
    }
    Keys::parse(&secret).map_err(|e| anyhow!("invalid ENCRYPTION_SECRET: {e}"))
}
