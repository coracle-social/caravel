//! Writing tenants.

use sqlx::PgExecutor;

use crate::commands;
use crate::error::Result;
use crate::models::tenants::Tenant;
use crate::queries;

/// Insert a tenant, or do nothing if one with its pubkey exists.
pub async fn insert_tenant(db: impl PgExecutor<'_>, tenant: &Tenant) -> Result<()> {
    sqlx::query(
        "INSERT INTO tenant (pubkey, return_url, nwc_url, created_at, stripe_customer_id)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (pubkey) DO NOTHING",
    )
    .bind(&tenant.pubkey)
    .bind(&tenant.return_url)
    .bind(&tenant.nwc_url)
    .bind(tenant.created_at)
    .bind(&tenant.stripe_customer_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Replace the tenant's encrypted NWC url, clearing the last NWC error.
pub async fn set_tenant_nwc_url(
    db: impl PgExecutor<'_>,
    pubkey: &str,
    nwc_url: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE tenant SET nwc_url = $1, nwc_error = NULL WHERE pubkey = $2")
        .bind(nwc_url)
        .bind(pubkey)
        .execute(db)
        .await?;
    Ok(())
}

/// Cache the tenant's Stripe payment method, or clear it, and clear the last
/// Stripe error.
pub async fn set_tenant_stripe_payment_method(
    db: impl PgExecutor<'_>,
    pubkey: &str,
    payment_method_id: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE tenant SET stripe_payment_method_id = $1, stripe_error = NULL WHERE pubkey = $2",
    )
    .bind(payment_method_id)
    .bind(pubkey)
    .execute(db)
    .await?;
    Ok(())
}

/// Set the billing anchor unless one is already set, returning the anchor in force.
pub async fn ensure_tenant_billing_anchor(
    db: impl PgExecutor<'_>,
    pubkey: &str,
    anchor: i64,
) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "UPDATE tenant SET billing_anchor = COALESCE(billing_anchor, $1)
         WHERE pubkey = $2 RETURNING billing_anchor",
    )
    .bind(anchor)
    .bind(pubkey)
    .fetch_one(db)
    .await?)
}

pub async fn set_tenant_nwc_error(
    db: impl PgExecutor<'_>,
    pubkey: &str,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE tenant SET nwc_error = $1 WHERE pubkey = $2")
        .bind(error)
        .bind(pubkey)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn set_tenant_stripe_error(
    db: impl PgExecutor<'_>,
    pubkey: &str,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE tenant SET stripe_error = $1 WHERE pubkey = $2")
        .bind(error)
        .bind(pubkey)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn set_tenant_renewed_at(db: impl PgExecutor<'_>, pubkey: &str, at: i64) -> Result<()> {
    sqlx::query("UPDATE tenant SET renewed_at = $1 WHERE pubkey = $2")
        .bind(at)
        .bind(pubkey)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn set_tenant_overage_billed_at(
    db: impl PgExecutor<'_>,
    pubkey: &str,
    at: i64,
) -> Result<()> {
    sqlx::query("UPDATE tenant SET overage_billed_at = $1 WHERE pubkey = $2")
        .bind(at)
        .bind(pubkey)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn set_tenant_churned_at(
    db: impl PgExecutor<'_>,
    pubkey: &str,
    at: Option<i64>,
) -> Result<()> {
    sqlx::query("UPDATE tenant SET churned_at = $1 WHERE pubkey = $2")
        .bind(at)
        .bind(pubkey)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn set_tenant_stripe_account(
    db: impl PgExecutor<'_>,
    pubkey: &str,
    account_id: &str,
    ready: bool,
) -> Result<()> {
    sqlx::query(
        "UPDATE tenant SET stripe_account_id = $2, stripe_account_ready = $3 WHERE pubkey = $1",
    )
    .bind(pubkey)
    .bind(account_id)
    .bind(ready)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_tenant_banned_at(
    db: impl PgExecutor<'_>,
    pubkey: &str,
    at: Option<i64>,
) -> Result<()> {
    sqlx::query("UPDATE tenant SET banned_at = $1 WHERE pubkey = $2")
        .bind(at)
        .bind(pubkey)
        .execute(db)
        .await?;
    Ok(())
}

/// Void a tenant's open invoices, forgive their unbilled charges and purge their
/// relays, returning the relays purged. Purges stay unbilled, so their credit is
/// still owed if the tenant returns.
pub async fn forgive_and_purge_in_tx(
    conn: &mut sqlx::PgConnection,
    pubkey: &str,
    now: i64,
) -> Result<Vec<String>> {
    commands::activity::forgive_unbilled_activity_for_tenant(&mut *conn, pubkey, now).await?;
    commands::ledger::void_open_invoices_for_tenant(conn, pubkey, now).await?;

    let mut purged = Vec::new();
    for relay in queries::relays::list_relays_for_tenant(&mut *conn, pubkey).await? {
        if !relay.is_purged() {
            commands::relays::purge_relay_in_tx(conn, &relay, now).await?;
            purged.push(relay.id);
        }
    }
    Ok(purged)
}
