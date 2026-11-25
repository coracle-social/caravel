use std::collections::HashSet;

use serde_json::Value;
use sha2::{Digest, Sha256};

/// An item's rank under a salt, the same on every relay: `(n + 1) / 2^64`, where `n`
/// is the first 8 bytes of SHA-256 over the salt and then the item's key.
pub fn rank(salt: &str, key: &[u8]) -> f64 {
    let digest = Sha256::new()
        .chain_update(salt.as_bytes())
        .chain_update(key)
        .finalize();
    let n = u64::from_be_bytes(digest[..8].try_into().expect("a digest is 32 bytes"));

    (u128::from(n) + 1) as f64 / 2f64.powi(64)
}

/// The lowest-ranked items offered so far, at most `size` of them, each with the
/// entry a response carries for it.
pub struct Sampler {
    size: usize,
    items: Vec<(f64, String, Value)>,
    keys: HashSet<String>,
    threshold: f64,
}

impl Sampler {
    /// A sampler that keeps the `size` lowest-ranked items.
    pub fn new(size: usize) -> Self {
        Self {
            size,
            items: Vec::new(),
            keys: HashSet::new(),
            threshold: f64::INFINITY,
        }
    }

    /// Consider an item, building its entry only if it ranks low enough to keep.
    pub fn offer(&mut self, rank: f64, key: &str, entry: impl FnOnce() -> Value) {
        if rank >= self.threshold || self.keys.contains(key) {
            return;
        }

        self.items.push((rank, key.to_string(), entry()));
        self.keys.insert(key.to_string());

        if self.items.len() >= self.size * 2 {
            self.trim();
        }
    }

    /// The kept entries, lowest rank first.
    pub fn drain(mut self) -> Vec<Value> {
        self.trim();
        self.items.into_iter().map(|(_, _, entry)| entry).collect()
    }

    /// Keep only the `size` lowest-ranked items, and once there are that many, ignore
    /// any item ranked above the last of them.
    fn trim(&mut self) {
        self.items.sort_by(|a, b| a.0.total_cmp(&b.0));
        self.items.truncate(self.size);
        self.keys = self.items.iter().map(|(_, key, _)| key.clone()).collect();
        if self.items.len() == self.size {
            self.threshold = self.items[self.size - 1].0;
        }
    }
}
