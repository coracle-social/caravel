//! Recording uploads to a relay and their deletion.

use sqlx::PgConnection;

use crate::queries;
use caravel_util::error::Result;
use caravel_domain::media::{Blob, Deleted};

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

    queries::media::get_blob(conn, relay_id, sha256)
        .await?
        .ok_or_else(|| caravel_util::error::Error::internal("blob missing after upload"))
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
