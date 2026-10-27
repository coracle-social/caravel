//! Media: the blobs a relay's members upload over Blossom. A blob is stored once
//! per relay however many members upload it, and each uploader owns it until they
//! delete it; the last owner's deletion removes it.

use serde::{Deserialize, Serialize};

/// The largest blob a relay takes.
pub const MAX_BLOB_SIZE: u64 = 10 * 1024 * 1024;

/// A file a relay holds.
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Blob {
    pub sha256: String,
    /// The content type its first uploader gave it.
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub content_type: String,
    /// Its size in bytes.
    pub size: i64,
    /// When it was first uploaded, or, in one uploader's list, when they uploaded it.
    pub created_at: i64,
}

/// What deleting an upload did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deleted {
    /// The pubkey owned no such blob.
    NotOwned,
    /// Others still own it, so its bytes stay.
    Shared,
    /// That was the last owner, so the blob went with it.
    Last,
}
