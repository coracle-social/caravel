use anyhow::{Result, anyhow};

use crate::db::pool;
use crate::models::{Activity, Bolt11, Checkout, Invoice, InvoiceItem, Plan, Relay, Tenant};

fn select_tenant(tail: &str) -> String {
    format!("SELECT * FROM tenant {tail}")
}

fn select_relay(tail: &str) -> String {
    format!("SELECT * FROM relay {tail}")
}

fn select_activity(tail: &str) -> String {
    format!("SELECT * FROM activity {tail}")
}

// --- Plans ---

pub fn list_plans() -> Vec<Plan> {
    vec![
        Plan {
            id: "free".to_string(),
            name: "Free".to_string(),
            amount: 0,
            members: Some(10),
            blossom: false,
            livekit: false,
        },
        Plan {
            id: "basic".to_string(),
            name: "Basic".to_string(),
            amount: 500,
            members: Some(100),
            blossom: true,
            livekit: true,
        },
        Plan {
            id: "growth".to_string(),
            name: "Growth".to_string(),
            amount: 2500,
            members: None,
            blossom: true,
            livekit: true,
        },
    ]
}

pub fn get_plan(plan_id: &str) -> Result<Plan> {
    list_plans()
        .into_iter()
        .find(|p| p.id == plan_id)
        .ok_or_else(|| anyhow!("plan not found: {plan_id}"))
}

// --- Tenants ---

pub async fn list_tenants() -> Result<Vec<Tenant>> {
    Ok(sqlx::query_as::<_, Tenant>(&select_tenant(""))
        .fetch_all(pool())
        .await?)
}

pub async fn get_tenant(pubkey: &str) -> Result<Option<Tenant>> {
    Ok(
        sqlx::query_as::<_, Tenant>(&select_tenant("WHERE pubkey = ?"))
            .bind(pubkey)
            .fetch_optional(pool())
            .await?,
    )
}

// --- Relays ---

pub async fn list_relays() -> Result<Vec<Relay>> {
    Ok(sqlx::query_as::<_, Relay>(&select_relay(""))
        .fetch_all(pool())
        .await?)
}

pub async fn list_relays_pending_sync() -> Result<Vec<Relay>> {
    Ok(
        sqlx::query_as::<_, Relay>(&select_relay("WHERE synced = 0 OR TRIM(sync_error) != ''"))
            .fetch_all(pool())
            .await?,
    )
}

pub async fn list_relays_for_tenant(tenant_pubkey: &str) -> Result<Vec<Relay>> {
    Ok(
        sqlx::query_as::<_, Relay>(&select_relay("WHERE tenant_pubkey = ?"))
            .bind(tenant_pubkey)
            .fetch_all(pool())
            .await?,
    )
}

pub async fn get_relay(id: &str) -> Result<Option<Relay>> {
    Ok(sqlx::query_as::<_, Relay>(&select_relay("WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool())
        .await?)
}

/// The relay's plan immediately before `before`, read from the most recent
/// relay-activity snapshot with `created_at < before`. Billing uses this as
/// the `old` side of a plan-change delta.
pub async fn get_relay_plan_before(relay_id: &str, before: i64) -> Result<Option<String>> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT json_extract(snapshot, '$.plan') FROM activity
         WHERE resource_id = ?
           AND resource_type = 'relay'
           AND created_at < ?
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .bind(relay_id)
    .bind(before)
    .fetch_optional(pool())
    .await?)
}

// --- Invoices ---

pub async fn get_invoice(invoice_id: &str) -> Result<Option<Invoice>> {
    Ok(
        sqlx::query_as::<_, Invoice>("SELECT * FROM invoice WHERE id = ?")
            .bind(invoice_id)
            .fetch_optional(pool())
            .await?,
    )
}

pub async fn list_invoices() -> Result<Vec<Invoice>> {
    Ok(
        sqlx::query_as::<_, Invoice>("SELECT * FROM invoice ORDER BY created_at DESC")
            .fetch_all(pool())
            .await?,
    )
}

pub async fn list_invoices_for_tenant(tenant_pubkey: &str) -> Result<Vec<Invoice>> {
    Ok(sqlx::query_as::<_, Invoice>(
        "SELECT * FROM invoice WHERE tenant_pubkey = ? ORDER BY created_at DESC",
    )
    .bind(tenant_pubkey)
    .fetch_all(pool())
    .await?)
}

/// The line items claimed onto an invoice, oldest first. Used to render an
/// invoice's contents (and its downloadable copy) from what was actually billed.
pub async fn list_invoice_items_for_invoice(invoice_id: &str) -> Result<Vec<InvoiceItem>> {
    Ok(sqlx::query_as::<_, InvoiceItem>(
        "SELECT * FROM invoice_item WHERE invoice_id = ? ORDER BY created_at ASC",
    )
    .bind(invoice_id)
    .fetch_all(pool())
    .await?)
}

/// A tenant's outstanding line items — created but not yet claimed onto an
/// invoice — oldest first. These are exactly what `create_invoice` would bill,
/// and what a draft invoice presents before the balance is cut.
pub async fn list_unbilled_invoice_items(tenant_pubkey: &str) -> Result<Vec<InvoiceItem>> {
    Ok(sqlx::query_as::<_, InvoiceItem>(
        "SELECT * FROM invoice_item
         WHERE tenant_pubkey = ? AND invoice_id IS NULL AND voided_at IS NULL
         ORDER BY created_at ASC",
    )
    .bind(tenant_pubkey)
    .fetch_all(pool())
    .await?)
}

/// A tenant's open invoices — neither paid nor voided — oldest first. Dunning
/// retries each and treats the oldest one's `created_at` as the grace-period start.
pub async fn list_open_invoices(tenant_pubkey: &str) -> Result<Vec<Invoice>> {
    Ok(sqlx::query_as::<_, Invoice>(
        "SELECT * FROM invoice
         WHERE tenant_pubkey = ? AND paid_at IS NULL AND voided_at IS NULL
         ORDER BY created_at ASC",
    )
    .bind(tenant_pubkey)
    .fetch_all(pool())
    .await?)
}

// --- Bolt11 ---

pub async fn get_bolt11_for_invoice(invoice_id: &str) -> Result<Option<Bolt11>> {
    Ok(sqlx::query_as::<_, Bolt11>(
        "SELECT * FROM bolt11 WHERE invoice_id = ? ORDER BY created_at DESC LIMIT 1",
    )
    .bind(invoice_id)
    .fetch_optional(pool())
    .await?)
}

// --- Checkouts ---

/// The most recent Checkout session for an invoice, regardless of `settled_at`,
/// so a session can still be expired on Stripe after we've locally marked it
/// settled. Mirrors [`get_bolt11_for_invoice`]; callers gate on `settled_at`.
pub async fn get_checkout_for_invoice(invoice_id: &str) -> Result<Option<Checkout>> {
    Ok(sqlx::query_as::<_, Checkout>(
        "SELECT * FROM checkout WHERE invoice_id = ? ORDER BY created_at DESC LIMIT 1",
    )
    .bind(invoice_id)
    .fetch_optional(pool())
    .await?)
}

/// Every still-pending (unsettled) Checkout session for an invoice — the ones to
/// expire on Stripe once the invoice has been paid another way.
pub async fn list_pending_checkouts_for_invoice(invoice_id: &str) -> Result<Vec<Checkout>> {
    Ok(sqlx::query_as::<_, Checkout>(
        "SELECT * FROM checkout WHERE invoice_id = ? AND settled_at IS NULL ORDER BY created_at DESC",
    )
    .bind(invoice_id)
    .fetch_all(pool())
    .await?)
}

// --- Activity ---

/// Billable activity for a tenant not yet folded into an invoice. The
/// activity-type filter and the `billed_at IS NULL` guard live here so the
/// caller reconciles off a precise marker rather than a timestamp watermark.
/// Ordered oldest-first so line items and proration apply in event order.
pub async fn list_billable_activity(tenant_pubkey: &str) -> Result<Vec<Activity>> {
    Ok(sqlx::query_as::<_, Activity>(&select_activity(
        "WHERE tenant_pubkey = ?
           AND billed_at IS NULL
           AND activity_type IN (
               'create_relay', 'update_relay', 'activate_relay', 'deactivate_relay',
               'unmark_relay_delinquent'
           )
         ORDER BY created_at ASC",
    ))
    .bind(tenant_pubkey)
    .fetch_all(pool())
    .await?)
}

/// The relay's most recent activity strictly before `before`, or `None` if it
/// had no activity yet — i.e. the relay didn't exist at that point. Billing
/// reads its snapshot to recover the relay's state as of a period boundary.
/// Strict `<` so a relay created exactly at the boundary isn't counted active
/// there (its own creation charge covers that period).
pub async fn get_latest_relay_activity_before(
    relay_id: &str,
    before: i64,
) -> Result<Option<Activity>> {
    Ok(sqlx::query_as::<_, Activity>(&select_activity(
        "WHERE resource_id = ?
           AND resource_type = 'relay'
           AND created_at < ?
         ORDER BY created_at DESC
         LIMIT 1",
    ))
    .bind(relay_id)
    .bind(before)
    .fetch_optional(pool())
    .await?)
}

pub async fn list_activity_for_resource(resource_id: &str) -> Result<Vec<Activity>> {
    Ok(sqlx::query_as::<_, Activity>(&select_activity(
        "WHERE resource_id = ? ORDER BY created_at DESC",
    ))
    .bind(resource_id)
    .fetch_all(pool())
    .await?)
}

pub async fn get_latest_activity_for_resource_and_type(
    resource_id: &str,
    activity_type: &str,
) -> Result<Option<Activity>> {
    Ok(sqlx::query_as::<_, Activity>(&select_activity(
        "WHERE resource_id = ? AND activity_type = ? ORDER BY created_at DESC LIMIT 1",
    ))
    .bind(resource_id)
    .bind(activity_type)
    .fetch_optional(pool())
    .await?)
}
