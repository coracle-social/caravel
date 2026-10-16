//! Media: the blobs a relay's members upload over Blossom. A blob is stored once
//! per relay however many members upload it, and each uploader owns it until
//! they delete it; the last owner's deletion removes it.

use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgExecutor};

use crate::error::Result;

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

/// Where a relay keeps a blob's bytes.
pub fn blob_key(relay_id: &str, sha256: &str) -> String {
    format!("{relay_id}/{sha256}")
}

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

/// Record an upload: the blob, if it is new to the relay, and its uploader as an
/// owner. Returns the blob as stored.
pub async fn record_upload(
    conn: &mut PgConnection,
    relay_id: &str,
    sha256: &str,
    content_type: &str,
    size: i64,
    pubkey: &str,
    now: i64,
) -> Result<Blob> {
    sqlx::query(
        "INSERT INTO blob (relay_id, sha256, type, size, created_at) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (relay_id, sha256) DO NOTHING",
    )
    .bind(relay_id)
    .bind(sha256)
    .bind(content_type)
    .bind(size)
    .bind(now)
    .execute(&mut *conn)
    .await?;

    sqlx::query(
        "INSERT INTO blob_owner (relay_id, blob_sha256, pubkey, created_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT DO NOTHING",
    )
    .bind(relay_id)
    .bind(sha256)
    .bind(pubkey)
    .bind(now)
    .execute(&mut *conn)
    .await?;

    get_blob(conn, relay_id, sha256)
        .await?
        .ok_or_else(|| crate::Error::internal("blob missing after upload"))
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

/// What deleting an upload did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deleted {
    /// The pubkey owned no such blob.
    NotOwned,
    /// Others still own it, so its bytes stay.
    Shared,
    /// That was the last owner, so its bytes should go.
    Last,
}

/// Take a pubkey's ownership of a blob away, removing the blob with its last owner.
pub async fn delete_upload(
    conn: &mut PgConnection,
    relay_id: &str,
    sha256: &str,
    pubkey: &str,
) -> Result<Deleted> {
    let removed = sqlx::query(
        "DELETE FROM blob_owner WHERE relay_id = $1 AND blob_sha256 = $2 AND pubkey = $3",
    )
    .bind(relay_id)
    .bind(sha256)
    .bind(pubkey)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    if removed == 0 {
        return Ok(Deleted::NotOwned);
    }

    let orphaned = sqlx::query(
        "DELETE FROM blob WHERE relay_id = $1 AND sha256 = $2
           AND NOT EXISTS (SELECT 1 FROM blob_owner WHERE relay_id = $1 AND blob_sha256 = $2)",
    )
    .bind(relay_id)
    .bind(sha256)
    .execute(conn)
    .await?
    .rows_affected();

    Ok(if orphaned > 0 {
        Deleted::Last
    } else {
        Deleted::Shared
    })
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
