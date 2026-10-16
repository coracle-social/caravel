//! Tenants: the pubkeys that host relays on the platform and pay for them.

use sqlx::{PgConnection, PgExecutor};

use crate::error::{Error, Result};

#[derive(Debug, Default, Clone, sqlx::FromRow)]
pub struct Tenant {
    pub pubkey: String,
    /// Where the robot's DMs send this tenant: their account page in whichever
    /// client they signed up through, supplied at creation. Used as-is for the
    /// churn DM, and with `?invoice=<id>` appended for a payment reminder.
    pub return_url: String,
    /// The tenant's NWC url, encrypted with the platform key, if they set one.
    pub nwc_url: Option<String>,
    /// Last NWC auto-payment error, or `None` when the wallet last paid or has
    /// never been tried. Shown as a warning; it never blocks a retry. Cleared when
    /// the tenant updates their NWC url.
    pub nwc_error: Option<String>,
    /// Last Stripe auto-payment error, with the same semantics as `nwc_error`.
    pub stripe_error: Option<String>,
    pub created_at: i64,
    /// When the tenant's first billable activity happened, which their monthly
    /// billing periods are counted from. `None` until then.
    pub billing_anchor: Option<i64>,
    pub stripe_customer_id: String,
    /// The saved Stripe payment method collection charges, or `None` before the
    /// tenant adds a card in the Stripe portal.
    pub stripe_payment_method_id: Option<String>,
    /// `period_start` of the latest period the tenant was renewed for: the
    /// per-period renewal idempotency marker.
    pub renewed_at: Option<i64>,
    /// `period_start` of the latest closed period whose metered overage was billed.
    /// Overage is billed once its period is over, so this trails `renewed_at`.
    pub overage_billed_at: Option<i64>,
    /// When the tenant was churned for an invoice unpaid past the grace period. Its
    /// relays are delinquent while this is set. Cleared when the tenant has new
    /// billable activity, which voids the then-open invoices.
    pub churned_at: Option<i64>,
    /// When an admin banned the tenant. A ban blocks relay creation, forgives
    /// outstanding charges and purges every relay; unbanning restores none of it.
    pub banned_at: Option<i64>,
    /// The Stripe Connect account the tenant's members pay into, once they start
    /// onboarding.
    pub stripe_account_id: Option<String>,
    /// Whether that account can take charges and receive payouts, as Stripe last
    /// said.
    pub stripe_account_ready: bool,
}

impl Tenant {
    /// The Connect account the tenant's members can pay into now, if any.
    pub fn payout_account(&self) -> Option<&str> {
        self.stripe_account_id
            .as_deref()
            .filter(|_| self.stripe_account_ready)
    }
}

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
