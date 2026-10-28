//! Hex encoding and hashing, the shapes keys, event ids, hashes and opaque
//! tokens take on the wire.

use sha2::{Digest, Sha256};

/// An unguessable token, hex encoded.
pub fn random_token() -> String {
    hex::encode(rand::random::<[u8; 32]>())
}

/// Whether a string is 64 hex characters, as pubkeys, event ids and hashes are.
pub fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn sha256_hex(input: &[u8]) -> String {
    hex::encode(Sha256::digest(input))
}
