//! Reading relays, and finding the one a host belongs to.

use sqlx::{PgConnection, PgExecutor, Postgres, QueryBuilder};

use caravel_util::error::{Error, Result};

use super::Relay;

pub mod activity;
pub mod membership;
pub mod usage;

const SELECT_RELAY: &str = "SELECT relay.*, ARRAY(
    SELECT host FROM host_redirect WHERE host_redirect.relay_id = relay.id ORDER BY host
) AS redirect_from FROM relay";

pub async fn list_relays(db: impl PgExecutor<'_>) -> Result<Vec<Relay>> {
    Ok(
        sqlx::query_as::<_, Relay>(&format!("{SELECT_RELAY} ORDER BY created_at DESC"))
            .fetch_all(db)
            .await?,
    )
}

/// A page of every relay on the platform, newest first with ties broken by id,
/// starting after the relay `before` names.
pub async fn page_relays(
    db: impl PgExecutor<'_>,
    before: Option<&str>,
    limit: i64,
) -> Result<Vec<Relay>> {
    let mut query = QueryBuilder::<Postgres>::new(SELECT_RELAY);
    if let Some(before) = before {
        query
            .push(" WHERE (created_at, id) < (SELECT created_at, id FROM relay WHERE id = ")
            .push_bind(before.to_string())
            .push(")");
    }
    query
        .push(" ORDER BY created_at DESC, id DESC LIMIT ")
        .push_bind(limit);

    Ok(query.build_query_as::<Relay>().fetch_all(db).await?)
}

pub async fn list_relays_for_tenant(
    db: impl PgExecutor<'_>,
    tenant_pubkey: &str,
) -> Result<Vec<Relay>> {
    Ok(sqlx::query_as::<_, Relay>(&format!(
        "{SELECT_RELAY} WHERE tenant_pubkey = $1 ORDER BY created_at DESC"
    ))
    .bind(tenant_pubkey)
    .fetch_all(db)
    .await?)
}

pub async fn get_relay(db: impl PgExecutor<'_>, id: &str) -> Result<Option<Relay>> {
    Ok(
        sqlx::query_as::<_, Relay>(&format!("{SELECT_RELAY} WHERE id = $1"))
            .bind(id)
            .fetch_optional(db)
            .await?,
    )
}

/// A relay that must exist.
pub async fn require_relay(db: impl PgExecutor<'_>, id: &str) -> Result<Relay> {
    get_relay(db, id)
        .await?
        .ok_or(Error::NotFound("relay not found"))
}

/// Read a relay and lock its row until the transaction ends, which serializes its
/// joins and status changes.
pub async fn lock_relay(conn: &mut PgConnection, id: &str) -> Result<Relay> {
    sqlx::query_as::<_, Relay>(&format!("{SELECT_RELAY} WHERE id = $1 FOR UPDATE"))
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or(Error::NotFound("relay not found"))
}

/// The live relay a host belongs to: as its canonical host, its verified custom
/// domain, or one of the hosts that redirect to it.
pub async fn get_relay_by_host(db: impl PgExecutor<'_>, host: &str) -> Result<Option<Relay>> {
    let (subdomain, domain) = host.split_once('.').unwrap_or((host, ""));

    Ok(sqlx::query_as::<_, Relay>(&format!(
        "{SELECT_RELAY} WHERE purged_at IS NULL AND (
            (subdomain = $1 AND domain = $2)
            OR (custom_domain = $3 AND custom_domain_verified)
            OR id = (SELECT relay_id FROM host_redirect WHERE host = $3)
        )"
    ))
    .bind(subdomain)
    .bind(domain)
    .bind(host)
    .fetch_optional(db)
    .await?)
}

/// Whether a live relay other than `relay_id` has verified `custom_domain`.
pub async fn is_custom_domain_taken(
    db: impl PgExecutor<'_>,
    custom_domain: &str,
    relay_id: &str,
) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM relay
            WHERE custom_domain = $1 AND custom_domain_verified AND purged_at IS NULL AND id <> $2
        )",
    )
    .bind(custom_domain)
    .bind(relay_id)
    .fetch_one(db)
    .await?)
}

pub async fn list_relays_with_custom_domain(
    db: impl PgExecutor<'_>,
    verified: bool,
) -> Result<Vec<Relay>> {
    Ok(sqlx::query_as::<_, Relay>(&format!(
        "{SELECT_RELAY} WHERE custom_domain IS NOT NULL AND custom_domain_verified = $1 AND purged_at IS NULL"
    ))
    .bind(verified)
    .fetch_all(db)
    .await?)
}

/// Purged relays whose data is still to be erased.
pub async fn list_unerased_relays(db: impl PgExecutor<'_>) -> Result<Vec<Relay>> {
    Ok(sqlx::query_as::<_, Relay>(&format!(
        "{SELECT_RELAY} WHERE purged_at IS NOT NULL AND erased_at IS NULL"
    ))
    .fetch_all(db)
    .await?)
}
