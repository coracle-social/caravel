//! A blob a member uploaded, as the moderation feed shows it.

use serde::Serialize;

/// A blob a member uploaded.
#[derive(Clone, Debug, Serialize)]
pub struct Upload {
    pub sha256: String,
    /// Where the relay serves it.
    pub url: String,
    #[serde(rename = "type")]
    pub content_type: String,
    pub size: i64,
    /// The uploader.
    pub pubkey: String,
    pub created_at: i64,
}
