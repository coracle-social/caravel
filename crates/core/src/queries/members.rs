//! Reading a relay's members.

use sqlx::PgExecutor;

use crate::error::Result;
use crate::models::members::RelayMember;

pub async fn list_members(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<RelayMember>> {
    Ok(sqlx::query_as::<_, RelayMember>(
        "SELECT pubkey, source, roles, created_at FROM relay_member
         WHERE relay_id = $1 ORDER BY created_at, pubkey",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

pub async fn get_member(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    pubkey: &str,
) -> Result<Option<RelayMember>> {
    Ok(sqlx::query_as::<_, RelayMember>(
        "SELECT pubkey, source, roles, created_at FROM relay_member WHERE relay_id = $1 AND pubkey = $2",
    )
    .bind(relay_id)
    .bind(pubkey)
    .fetch_optional(db)
    .await?)
}

pub async fn count_members(db: impl PgExecutor<'_>, relay_id: &str) -> Result<i64> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM relay_member WHERE relay_id = $1")
            .bind(relay_id)
            .fetch_one(db)
            .await?,
    )
}
