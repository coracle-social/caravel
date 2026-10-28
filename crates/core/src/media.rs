//! Media: the blobs a relay's members upload over Blossom, their bytes in the blob
//! store and their records in the database, kept in step.

use bytes::Bytes;

use crate::Core;
use caravel_db::commands;
use caravel_domain as domain;
use caravel_domain::error::Result;
use caravel_domain::media::{Blob, Deleted};

/// Keep an upload: store its bytes, then record the blob, if it is new to the
/// relay, and its uploader as an owner. Returns the blob as stored.
pub async fn keep_upload(
    core: &Core,
    relay_id: &str,
    sha256: &str,
    content_type: &str,
    pubkey: &str,
    bytes: Bytes,
) -> Result<Blob> {
    let key = domain::media::blob_key(relay_id, sha256);
    let size = bytes.len() as i64;
    // Held until recorded, so the last owner's deletion can't remove these bytes meanwhile.
    let _lock = core.blobs.lock(&key).await;

    core.blobs.put(&key, bytes).await?;
    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            commands::media::record_upload(conn, relay_id, sha256, content_type, size, pubkey, now)
                .await
        })
        .await
}

/// Take a pubkey's ownership of a blob away, removing the blob and its bytes with
/// its last owner. Bytes that fail to delete are logged, since the blob is gone.
pub async fn delete_upload(
    core: &Core,
    relay_id: &str,
    sha256: &str,
    pubkey: &str,
) -> Result<Deleted> {
    let key = domain::media::blob_key(relay_id, sha256);
    let _lock = core.blobs.lock(&key).await;

    let deleted = core
        .db
        .with_tx(async |conn| commands::media::delete_upload(conn, relay_id, sha256, pubkey).await)
        .await?;
    if deleted == Deleted::Last
        && let Err(error) = core.blobs.delete(&key).await
    {
        tracing::warn!(relay = relay_id, error = %error, "failed to delete a blob's bytes");
    }
    Ok(deleted)
}
