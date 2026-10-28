//! Reading a relay's media.

use sqlx::PgExecutor;

use caravel_util::error::Result;
use caravel_domain::media::Blob;

pub async fn get_blob(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    sha256: &str,
) -> Result<Option<Blob>> {
    Ok(sqlx::query_as::<_, Blob>(
        "SELECT sha256, type, size, created_at FROM blob WHERE relay_id = $1 AND sha256 = $2",
    )
    .bind(relay_id)
    .bind(sha256)
    .fetch_optional(db)
    .await?)
}

/// The blobs a pubkey has uploaded to a relay, newest first, with when each was
/// uploaded.
pub async fn list_blobs_for_pubkey(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    pubkey: &str,
) -> Result<Vec<Blob>> {
    Ok(sqlx::query_as::<_, Blob>(
        "SELECT b.sha256, b.type, b.size, o.created_at FROM blob_owner o
         JOIN blob b ON b.relay_id = o.relay_id AND b.sha256 = o.blob_sha256
         WHERE o.relay_id = $1 AND o.pubkey = $2
         ORDER BY o.created_at DESC",
    )
    .bind(relay_id)
    .bind(pubkey)
    .fetch_all(db)
    .await?)
}

/// What a relay's blobs occupy, each counted once however many members own it.
pub async fn storage_bytes(db: impl PgExecutor<'_>, relay_id: &str) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(size), 0)::bigint FROM blob WHERE relay_id = $1",
    )
    .bind(relay_id)
    .fetch_one(db)
    .await?)
}
