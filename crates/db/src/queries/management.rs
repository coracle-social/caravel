//! Reading the NIP-86 methods granted on a relay.

use sqlx::PgExecutor;

use caravel_util::error::Result;

/// Every pubkey with methods granted individually, and its methods.
pub async fn list_method_grants(
    db: impl PgExecutor<'_>,
    relay_id: &str,
) -> Result<Vec<(String, Vec<String>)>> {
    Ok(sqlx::query_as::<_, (String, Vec<String>)>(
        "SELECT pubkey, array_agg(method ORDER BY created_at, method) FROM method_grant
         WHERE relay_id = $1 GROUP BY pubkey ORDER BY pubkey",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}
