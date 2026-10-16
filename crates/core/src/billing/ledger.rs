//! The billing ledger: line items, the invoices that claim them, and the payment
//! attempts made against an invoice.

use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgExecutor};

use crate::billing::period::BillingPeriod;
use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "text", rename_all = "lowercase")]
pub enum InvoiceMethod {
    Nwc,
    Stripe,
    /// A Lightning invoice paid out of band, such as by scanning its QR code.
    Oob,
    /// Comped: issued paid and never collected.
    Comp,
}

/// A tenant's bill for one period. It is open while `paid_at` and `voided_at` are
/// both null, paid once `paid_at` is set, and void once `voided_at` is.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Invoice {
    pub id: String,
    pub tenant_pubkey: String,
    /// The total owed, fixed when the invoice claims its line items.
    pub amount: i64,
    pub period_start: i64,
    pub period_end: i64,
    pub created_at: i64,
    pub paid_at: Option<i64>,
    pub voided_at: Option<i64>,
    /// When the manual-payment reminder DM was last sent for this invoice.
    pub notified_at: Option<i64>,
    /// How the invoice was paid, set when it is marked paid.
    pub method: Option<InvoiceMethod>,
}

impl Invoice {
    pub fn is_open(&self) -> bool {
        self.paid_at.is_none() && self.voided_at.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct InvoiceItem {
    pub id: String,
    /// `None` while outstanding; set once an invoice claims the item.
    pub invoice_id: Option<String>,
    /// The activity the item prices, or `None` for renewals and overage.
    pub activity_id: Option<String>,
    pub tenant_pubkey: String,
    pub relay_id: String,
    pub plan_id: String,
    pub amount: i64,
    pub description: String,
    pub created_at: i64,
    /// When the item was forgiven, because the tenant churned or came back, so it
    /// is never billed or carried into a later invoice.
    pub voided_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Bolt11 {
    pub id: String,
    pub invoice_id: String,
    pub lnbc: String,
    pub msats: i64,
    pub created_at: i64,
    pub expires_at: i64,
    pub settled_at: Option<i64>,
}

/// A hosted Stripe Checkout session opened to pay an invoice on-session, so the
/// tenant can clear a 3D Secure challenge. `session_id` is Stripe's `cs_…` id and
/// `url` the hosted page.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Checkout {
    pub id: String,
    pub invoice_id: String,
    pub session_id: String,
    pub url: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub settled_at: Option<i64>,
}

/// A write-ahead record of an off-session card charge, inserted before the Stripe
/// call. A retry after a lost settle re-confirms the same idempotent PaymentIntent
/// on `payment_method_id`; `payment_intent_id` is the Stripe `pi_…` that settled it.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Intent {
    pub id: String,
    pub invoice_id: String,
    pub payment_method_id: String,
    pub payment_intent_id: Option<String>,
    pub created_at: i64,
    pub settled_at: Option<i64>,
}

// --- Invoices ---

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

/// Claim a tenant's outstanding items onto a new invoice, if they come to more
/// than the minimum an invoice is cut for. A comped invoice is issued paid.
pub async fn insert_invoice_for_outstanding_items(
    conn: &mut PgConnection,
    tenant_pubkey: &str,
    period: &BillingPeriod,
    comped: bool,
    now: i64,
) -> Result<Option<Invoice>> {
    let total = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(amount), 0)::bigint FROM invoice_item
         WHERE tenant_pubkey = $1 AND invoice_id IS NULL AND voided_at IS NULL",
    )
    .bind(tenant_pubkey)
    .fetch_one(&mut *conn)
    .await?;

    // Up to $1 carries forward, so a small credit can't drop an invoice under Stripe's $0.50.
    if total <= 100 {
        return Ok(None);
    }

    let (paid_at, method) = if comped {
        (Some(now), Some(InvoiceMethod::Comp))
    } else {
        (None, None)
    };

    let invoice = sqlx::query_as::<_, Invoice>(
        "INSERT INTO invoice (id, tenant_pubkey, amount, period_start, period_end, created_at, paid_at, method)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING *",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(tenant_pubkey)
    .bind(total)
    .bind(period.start)
    .bind(period.end)
    .bind(now)
    .bind(paid_at)
    .bind(method)
    .fetch_one(&mut *conn)
    .await?;

    sqlx::query(
        "UPDATE invoice_item SET invoice_id = $1
         WHERE tenant_pubkey = $2 AND invoice_id IS NULL AND voided_at IS NULL",
    )
    .bind(&invoice.id)
    .bind(tenant_pubkey)
    .execute(&mut *conn)
    .await?;

    Ok(Some(invoice))
}

/// Mark an invoice paid, but only while it is open, so a late payment never flips
/// a voided invoice to paid or overwrites how a paid one was paid.
pub async fn mark_invoice_paid(
    db: impl PgExecutor<'_>,
    invoice_id: &str,
    method: InvoiceMethod,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE invoice SET method = $1, paid_at = $2
         WHERE id = $3 AND paid_at IS NULL AND voided_at IS NULL",
    )
    .bind(method)
    .bind(now)
    .bind(invoice_id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn mark_invoice_notified(
    db: impl PgExecutor<'_>,
    invoice_id: &str,
    now: i64,
) -> Result<()> {
    sqlx::query("UPDATE invoice SET notified_at = $1 WHERE id = $2")
        .bind(now)
        .bind(invoice_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Void a tenant's open invoices and every line item not on a paid invoice,
/// forgiving the balance so a credit can't bleed into a later invoice either.
pub async fn void_open_invoices_for_tenant(
    conn: &mut PgConnection,
    tenant_pubkey: &str,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE invoice SET voided_at = $1
         WHERE tenant_pubkey = $2 AND paid_at IS NULL AND voided_at IS NULL",
    )
    .bind(now)
    .bind(tenant_pubkey)
    .execute(&mut *conn)
    .await?;

    // After voiding the invoices, so their items count as not on a paid invoice.
    sqlx::query(
        "UPDATE invoice_item SET voided_at = $1
         WHERE tenant_pubkey = $2 AND voided_at IS NULL
           AND (invoice_id IS NULL OR invoice_id IN (SELECT id FROM invoice WHERE paid_at IS NULL))",
    )
    .bind(now)
    .bind(tenant_pubkey)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

// --- Line items ---

pub async fn insert_invoice_item(db: impl PgExecutor<'_>, item: &InvoiceItem) -> Result<()> {
    sqlx::query(
        "INSERT INTO invoice_item
            (id, invoice_id, activity_id, tenant_pubkey, relay_id, plan_id, amount, description, created_at, voided_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(&item.id)
    .bind(&item.invoice_id)
    .bind(&item.activity_id)
    .bind(&item.tenant_pubkey)
    .bind(&item.relay_id)
    .bind(&item.plan_id)
    .bind(item.amount)
    .bind(&item.description)
    .bind(item.created_at)
    .bind(item.voided_at)
    .execute(db)
    .await?;
    Ok(())
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

// --- Lightning ---

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

pub async fn insert_bolt11(
    db: impl PgExecutor<'_>,
    invoice_id: &str,
    lnbc: &str,
    msats: i64,
    expires_at: i64,
    now: i64,
) -> Result<Bolt11> {
    Ok(sqlx::query_as::<_, Bolt11>(
        "INSERT INTO bolt11 (id, invoice_id, lnbc, msats, created_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING *",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(invoice_id)
    .bind(lnbc)
    .bind(msats)
    .bind(now)
    .bind(expires_at)
    .fetch_one(db)
    .await?)
}

/// Stamp a Lightning invoice settled, keeping an earlier stamp.
pub async fn mark_bolt11_settled(db: impl PgExecutor<'_>, bolt11_id: &str, now: i64) -> Result<()> {
    sqlx::query("UPDATE bolt11 SET settled_at = $1 WHERE id = $2 AND settled_at IS NULL")
        .bind(now)
        .bind(bolt11_id)
        .execute(db)
        .await?;
    Ok(())
}

// --- Checkout ---

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

pub async fn insert_checkout(
    db: impl PgExecutor<'_>,
    invoice_id: &str,
    session_id: &str,
    url: &str,
    expires_at: i64,
    now: i64,
) -> Result<Checkout> {
    Ok(sqlx::query_as::<_, Checkout>(
        "INSERT INTO checkout (id, invoice_id, session_id, url, created_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING *",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(invoice_id)
    .bind(session_id)
    .bind(url)
    .bind(now)
    .bind(expires_at)
    .fetch_one(db)
    .await?)
}

/// Stamp a Checkout session settled, keeping an earlier stamp.
pub async fn mark_checkout_settled(
    db: impl PgExecutor<'_>,
    checkout_id: &str,
    now: i64,
) -> Result<()> {
    sqlx::query("UPDATE checkout SET settled_at = $1 WHERE id = $2 AND settled_at IS NULL")
        .bind(now)
        .bind(checkout_id)
        .execute(db)
        .await?;
    Ok(())
}

// --- Intents ---

/// The invoice's unsettled intent, or a new one charging `payment_method_id`. An
/// existing intent keeps its payment method, so a retry re-confirms the same charge.
pub async fn ensure_pending_intent(
    db: impl PgExecutor<'_>,
    invoice_id: &str,
    payment_method_id: &str,
    now: i64,
) -> Result<Intent> {
    Ok(sqlx::query_as::<_, Intent>(
        "INSERT INTO intent (id, invoice_id, payment_method_id, created_at)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (invoice_id) WHERE settled_at IS NULL
           DO UPDATE SET payment_method_id = intent.payment_method_id
         RETURNING *",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(invoice_id)
    .bind(payment_method_id)
    .bind(now)
    .fetch_one(db)
    .await?)
}

/// Stamp an intent settled by the PaymentIntent that confirmed it, keeping an
/// earlier stamp.
pub async fn mark_intent_settled(
    db: impl PgExecutor<'_>,
    intent_id: &str,
    payment_intent_id: &str,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE intent SET settled_at = $1, payment_intent_id = $2 WHERE id = $3 AND settled_at IS NULL",
    )
    .bind(now)
    .bind(payment_intent_id)
    .bind(intent_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Drop an intent whose charge didn't go through, so the next attempt starts on the
/// tenant's current payment method.
pub async fn delete_intent(db: impl PgExecutor<'_>, intent_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM intent WHERE id = $1")
        .bind(intent_id)
        .execute(db)
        .await?;
    Ok(())
}
