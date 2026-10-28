//! Writing the hosts a relay answers on and the hosts that redirect to it.

use sqlx::PgExecutor;

use caravel_util::error::Result;

/// Make `host` redirect to a relay, taking it from whichever relay it redirected to.
pub async fn add_host_redirect(
    db: impl PgExecutor<'_>,
    host: &str,
    relay_id: &str,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO host_redirect (host, relay_id, created_at) VALUES ($1, $2, $3)
         ON CONFLICT (host) DO UPDATE SET relay_id = EXCLUDED.relay_id, created_at = EXCLUDED.created_at",
    )
    .bind(host)
    .bind(relay_id)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

/// Stop redirecting to a relay the hosts it now answers on itself. Another
/// relay's redirects stay, since naming a domain doesn't prove holding it.
pub async fn remove_host_redirects(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    hosts: &[String],
) -> Result<()> {
    sqlx::query("DELETE FROM host_redirect WHERE relay_id = $1 AND host = ANY($2)")
        .bind(relay_id)
        .bind(hosts)
        .execute(db)
        .await?;
    Ok(())
}

/// Stop redirecting `host`, for a relay that has verified it as its own domain,
/// returning the relay it redirected to.
pub async fn take_host_redirect(db: impl PgExecutor<'_>, host: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("DELETE FROM host_redirect WHERE host = $1 RETURNING relay_id")
            .bind(host)
            .fetch_optional(db)
            .await?,
    )
}
