//! Media: the blobs a relay's members upload over Blossom, their bytes in the blob
//! store and their records in the database, kept in step.

use bytes::Bytes;

use crate::Core;
use caravel_relays::media::{Blob, Deleted};
use caravel_util::error::Result;

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
    let key = caravel_relays::media::blob_key(relay_id, sha256);
    let size = bytes.len() as i64;
    // Held until recorded, so the last owner's deletion can't remove these bytes meanwhile.
    let _lock = core.blobs.lock(&key).await;

    core.blobs.put(&key, bytes).await?;
    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            caravel_relays::media::commands::record_upload(
                conn,
                relay_id,
                sha256,
                content_type,
                size,
                pubkey,
                now,
            )
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
    let key = caravel_relays::media::blob_key(relay_id, sha256);
    let _lock = core.blobs.lock(&key).await;

    let deleted = core
        .db
        .with_tx(async |conn| {
            caravel_relays::media::commands::delete_upload(conn, relay_id, sha256, pubkey).await
        })
        .await?;
    if deleted == Deleted::Last
        && let Err(error) = core.blobs.delete(&key).await
    {
        tracing::warn!(relay = relay_id, error = %error, "failed to delete a blob's bytes");
    }
    Ok(deleted)
}

/// The blob with `sha256` on a relay, if it has one.
pub async fn get_blob(core: &Core, relay_id: &str, sha256: &str) -> Result<Option<Blob>> {
    caravel_relays::media::queries::get_blob(core.db.pool(), relay_id, sha256).await
}

/// The blobs a pubkey uploaded to a relay.
pub async fn list_blobs_for_pubkey(core: &Core, relay_id: &str, pubkey: &str) -> Result<Vec<Blob>> {
    caravel_relays::media::queries::list_blobs_for_pubkey(core.db.pool(), relay_id, pubkey).await
}
