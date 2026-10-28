//! Reading who holds which access to a relay: its members, bans, roles and claims.

use sqlx::{PgConnection, PgExecutor};

use caravel_util::error::{Error, Result};

use crate::access::{Ban, RelayMember, Role};

pub async fn list_banned_pubkeys(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<Ban>> {
    Ok(sqlx::query_as::<_, Ban>(
        "SELECT pubkey AS subject, reason, created_at FROM banned_pubkey
         WHERE relay_id = $1 ORDER BY created_at, pubkey",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

pub async fn list_banned_events(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<Ban>> {
    Ok(sqlx::query_as::<_, Ban>(
        "SELECT event_id AS subject, reason, created_at FROM banned_event
         WHERE relay_id = $1 ORDER BY created_at, event_id",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

/// The relay's claims that haven't been revoked.
pub async fn list_claims(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT code FROM claim WHERE relay_id = $1 AND revoked_at IS NULL ORDER BY created_at, code",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

/// Whether a claim exists and hasn't been revoked.
pub async fn is_live_claim(db: impl PgExecutor<'_>, relay_id: &str, code: &str) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM claim WHERE relay_id = $1 AND code = $2 AND revoked_at IS NULL)",
    )
    .bind(relay_id)
    .bind(code)
    .fetch_one(db)
    .await?)
}

pub async fn list_roles(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<Role>> {
    Ok(sqlx::query_as::<_, Role>(
        "SELECT id, label, description, hue, position FROM role WHERE relay_id = $1 ORDER BY position, id",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

pub async fn require_role(conn: &mut PgConnection, relay_id: &str, role_id: &str) -> Result<()> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM role WHERE relay_id = $1 AND id = $2)",
    )
    .bind(relay_id)
    .bind(role_id)
    .fetch_one(conn)
    .await?;

    if exists {
        Ok(())
    } else {
        Err(Error::conflict(
            "role-missing",
            format!("role {role_id:?} does not exist"),
        ))
    }
}

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
