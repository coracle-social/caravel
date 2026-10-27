//! Where a relay's media lives.

/// Where a relay keeps a blob's bytes.
pub fn blob_key(relay_id: &str, sha256: &str) -> String {
    format!("{relay_id}/{sha256}")
}
