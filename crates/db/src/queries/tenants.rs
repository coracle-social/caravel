//! Reading tenants.

use sqlx::{PgConnection, PgExecutor};

use caravel_util::error::{Error, Result};
use caravel_domain::tenants::Tenant;

pub async fn list_tenants(db: impl PgExecutor<'_>) -> Result<Vec<Tenant>> {
    Ok(
        sqlx::query_as::<_, Tenant>("SELECT * FROM tenant ORDER BY created_at DESC")
            .fetch_all(db)
            .await?,
    )
}

pub async fn get_tenant(db: impl PgExecutor<'_>, pubkey: &str) -> Result<Option<Tenant>> {
    Ok(
        sqlx::query_as::<_, Tenant>("SELECT * FROM tenant WHERE pubkey = $1")
            .bind(pubkey)
            .fetch_optional(db)
            .await?,
    )
}

/// A tenant that must exist.
pub async fn require_tenant(db: impl PgExecutor<'_>, pubkey: &str) -> Result<Tenant> {
    get_tenant(db, pubkey)
        .await?
        .ok_or(Error::NotFound("tenant not found"))
}

/// Read a tenant and lock its row until the transaction ends, which serializes
/// billing writes for one tenant.
pub async fn lock_tenant(conn: &mut PgConnection, pubkey: &str) -> Result<Tenant> {
    sqlx::query_as::<_, Tenant>("SELECT * FROM tenant WHERE pubkey = $1 FOR UPDATE")
        .bind(pubkey)
        .fetch_optional(conn)
        .await?
        .ok_or(Error::NotFound("tenant not found"))
}
