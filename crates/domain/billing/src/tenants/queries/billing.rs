//! Reading the billing ledger.

use sqlx::PgExecutor;

use crate::tenants::{Bolt11, Checkout, Invoice, InvoiceItem};
use caravel_util::error::{Error, Result};

pub async fn get_invoice(db: impl PgExecutor<'_>, id: &str) -> Result<Option<Invoice>> {
    Ok(
        sqlx::query_as::<_, Invoice>("SELECT * FROM invoice WHERE id = $1")
            .bind(id)
            .fetch_optional(db)
            .await?,
    )
}

/// An invoice that must exist.
pub async fn require_invoice(db: impl PgExecutor<'_>, id: &str) -> Result<Invoice> {
    get_invoice(db, id)
        .await?
        .ok_or(Error::NotFound("invoice not found"))
}

pub async fn list_invoices(db: impl PgExecutor<'_>) -> Result<Vec<Invoice>> {
    Ok(
        sqlx::query_as::<_, Invoice>("SELECT * FROM invoice ORDER BY created_at DESC, seq DESC")
            .fetch_all(db)
            .await?,
    )
}

pub async fn list_invoices_for_tenant(
    db: impl PgExecutor<'_>,
    tenant_pubkey: &str,
) -> Result<Vec<Invoice>> {
    Ok(sqlx::query_as::<_, Invoice>(
        "SELECT * FROM invoice WHERE tenant_pubkey = $1 ORDER BY created_at DESC, seq DESC",
    )
    .bind(tenant_pubkey)
    .fetch_all(db)
    .await?)
}

/// A tenant's open invoices, oldest first. Dunning retries each, and the oldest
/// one's `created_at` starts the grace period.
pub async fn list_open_invoices_for_tenant(
    db: impl PgExecutor<'_>,
    tenant_pubkey: &str,
) -> Result<Vec<Invoice>> {
    Ok(sqlx::query_as::<_, Invoice>(
        "SELECT * FROM invoice
         WHERE tenant_pubkey = $1 AND paid_at IS NULL AND voided_at IS NULL
         ORDER BY created_at, seq",
    )
    .bind(tenant_pubkey)
    .fetch_all(db)
    .await?)
}

/// The items billed on an invoice, oldest first.
pub async fn list_invoice_items_for_invoice(
    db: impl PgExecutor<'_>,
    invoice_id: &str,
) -> Result<Vec<InvoiceItem>> {
    Ok(sqlx::query_as::<_, InvoiceItem>(
        "SELECT * FROM invoice_item WHERE invoice_id = $1 ORDER BY created_at, seq",
    )
    .bind(invoice_id)
    .fetch_all(db)
    .await?)
}

/// A tenant's outstanding items, oldest first: what the next invoice would claim.
pub async fn list_outstanding_invoice_items_for_tenant(
    db: impl PgExecutor<'_>,
    tenant_pubkey: &str,
) -> Result<Vec<InvoiceItem>> {
    Ok(sqlx::query_as::<_, InvoiceItem>(
        "SELECT * FROM invoice_item
         WHERE tenant_pubkey = $1 AND invoice_id IS NULL AND voided_at IS NULL
         ORDER BY created_at, seq",
    )
    .bind(tenant_pubkey)
    .fetch_all(db)
    .await?)
}

/// An invoice's latest Lightning invoice.
pub async fn get_bolt11_for_invoice(
    db: impl PgExecutor<'_>,
    invoice_id: &str,
) -> Result<Option<Bolt11>> {
    Ok(sqlx::query_as::<_, Bolt11>(
        "SELECT * FROM bolt11 WHERE invoice_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(invoice_id)
    .fetch_optional(db)
    .await?)
}

/// An invoice's latest Checkout session, settled or not, so a session can still be
/// expired on Stripe after it was settled here.
pub async fn get_checkout_for_invoice(
    db: impl PgExecutor<'_>,
    invoice_id: &str,
) -> Result<Option<Checkout>> {
    Ok(sqlx::query_as::<_, Checkout>(
        "SELECT * FROM checkout WHERE invoice_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(invoice_id)
    .fetch_optional(db)
    .await?)
}

/// An invoice's unsettled Checkout sessions, to expire once it is paid another way.
pub async fn list_pending_checkouts_for_invoice(
    db: impl PgExecutor<'_>,
    invoice_id: &str,
) -> Result<Vec<Checkout>> {
    Ok(sqlx::query_as::<_, Checkout>(
        "SELECT * FROM checkout WHERE invoice_id = $1 AND settled_at IS NULL ORDER BY created_at DESC",
    )
    .bind(invoice_id)
    .fetch_all(db)
    .await?)
}

/// What a tenant's items no invoice has claimed come to.
pub async fn get_outstanding_total(db: impl PgExecutor<'_>, tenant_pubkey: &str) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(amount), 0)::bigint FROM invoice_item
         WHERE tenant_pubkey = $1 AND invoice_id IS NULL AND voided_at IS NULL",
    )
    .bind(tenant_pubkey)
    .fetch_one(db)
    .await?)
}
