//! Reading a relay's claims.

use sqlx::PgExecutor;

use caravel_util::error::Result;

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
