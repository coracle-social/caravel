//! Reading tenants.

use sqlx::{PgConnection, PgExecutor, Postgres, QueryBuilder};

use caravel_util::error::{Error, Result};

use super::Tenant;

pub mod billing;

pub async fn list_tenants(db: impl PgExecutor<'_>) -> Result<Vec<Tenant>> {
    Ok(
        sqlx::query_as::<_, Tenant>("SELECT * FROM tenant ORDER BY created_at DESC")
            .fetch_all(db)
            .await?,
    )
}

/// A page of every tenant on the platform, newest first with ties broken by pubkey,
/// starting after the tenant `before` names.
pub async fn page_tenants(
    db: impl PgExecutor<'_>,
    before: Option<&str>,
    limit: i64,
) -> Result<Vec<Tenant>> {
    let mut query = QueryBuilder::<Postgres>::new("SELECT * FROM tenant");
    if let Some(before) = before {
        query
            .push(" WHERE (created_at, pubkey) < (SELECT created_at, pubkey FROM tenant WHERE pubkey = ")
            .push_bind(before.to_string())
            .push(")");
    }
    query
        .push(" ORDER BY created_at DESC, pubkey DESC LIMIT ")
        .push_bind(limit);

    Ok(query.build_query_as::<Tenant>().fetch_all(db).await?)
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
