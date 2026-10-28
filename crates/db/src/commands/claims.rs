//! Creating and revoking a relay's claims.

use sqlx::PgExecutor;

use caravel_util::error::{Error, Result};

/// Register a claim, or bring a revoked one back. Creating a live claim again
/// does nothing.
pub async fn create_claim(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    code: &str,
    now: i64,
) -> Result<()> {
    if code.is_empty() {
        return Err(Error::invalid("claim-required", "claim is required"));
    }

    sqlx::query(
        "INSERT INTO claim (relay_id, code, created_at) VALUES ($1, $2, $3)
         ON CONFLICT (relay_id, code) DO UPDATE SET revoked_at = NULL",
    )
    .bind(relay_id)
    .bind(code)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

/// Stop a claim working, keeping its history.
pub async fn revoke_claim(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    code: &str,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE claim SET revoked_at = $3 WHERE relay_id = $1 AND code = $2 AND revoked_at IS NULL",
    )
    .bind(relay_id)
    .bind(code)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}
