use anyhow::Result;

use crate::models::{Activity, Bolt11, Invoice, InvoiceItem, Plan, Relay, Tenant};
use crate::db::pool;

fn select_tenant(tail: &str) -> String {
    format!("SELECT * FROM tenant {tail}")
}

fn select_relay(tail: &str) -> String {
    format!("SELECT * FROM relay {tail}")
}

fn select_activity(tail: &str) -> String {
    format!("SELECT * FROM activity {tail}")
}

// Plans

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

pub fn get_plan(plan_id: &str) -> Option<Plan> {
    list_plans().into_iter().find(|p| p.id == plan_id)
}

// Tenants

pub async fn list_tenants() -> Result<Vec<Tenant>> {
    Ok(sqlx::query_as::<_, Tenant>(&select_tenant(""))
        .fetch_all(pool())
        .await?)
}

pub async fn get_tenant(pubkey: &str) -> Result<Option<Tenant>> {
    Ok(sqlx::query_as::<_, Tenant>(&select_tenant("WHERE pubkey = ?"))
        .bind(pubkey)
        .fetch_optional(pool())
        .await?)
}

// Relays

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

pub async fn list_relays_for_tenant(tenant_id: &str) -> Result<Vec<Relay>> {
    Ok(sqlx::query_as::<_, Relay>(&select_relay("WHERE tenant = ?"))
        .bind(tenant_id)
        .fetch_all(pool())
        .await?)
}

pub async fn get_relay(id: &str) -> Result<Option<Relay>> {
    Ok(sqlx::query_as::<_, Relay>(&select_relay("WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool())
        .await?)
}

// Invoices

pub async fn get_invoice(invoice_id: &str) -> Result<Option<Invoice>> {
    Ok(sqlx::query_as::<_, Invoice>("SELECT * FROM invoice WHERE id = ?")
        .bind(invoice_id)
        .fetch_optional(pool())
        .await?)
}

pub async fn list_invoices(tenant_pubkey: &str) -> Result<Vec<Invoice>> {
    Ok(sqlx::query_as::<_, Invoice>(
        "SELECT * FROM invoice WHERE tenant_pubkey = ? ORDER BY created_at DESC",
    )
    .bind(tenant_pubkey)
    .fetch_all(pool())
    .await?)
}

pub async fn get_invoice_items_for_invoice(invoice_id: &str) -> Result<Vec<InvoiceItem>> {
    Ok(
        sqlx::query_as::<_, InvoiceItem>("SELECT * FROM invoice_item WHERE invoice_id = ?")
            .bind(invoice_id)
            .fetch_all(pool())
            .await?,
    )
}

pub async fn get_bolt11(bolt11_id: &str) -> Result<Option<Bolt11>> {
    Ok(sqlx::query_as::<_, Bolt11>("SELECT * FROM bolt11 WHERE id = ?")
        .bind(bolt11_id)
        .fetch_optional(pool())
        .await?)
}

pub async fn get_bolt11_for_invoice(invoice_id: &str) -> Result<Option<Bolt11>> {
    Ok(sqlx::query_as::<_, Bolt11>(
        "SELECT * FROM bolt11 WHERE invoice_id = ? ORDER BY created_at DESC LIMIT 1",
    )
    .bind(invoice_id)
    .fetch_optional(pool())
    .await?)
}

// Activity

/// Billable activity for a tenant not yet folded into an invoice. The
/// activity-type filter and the `billed_at IS NULL` guard live here so the
/// caller reconciles off a precise marker rather than a timestamp watermark.
/// Ordered oldest-first so line items and proration apply in event order.
pub async fn list_billable_activity_for_tenant(tenant_pubkey: &str) -> Result<Vec<Activity>> {
    Ok(sqlx::query_as::<_, Activity>(&select_activity(
        "WHERE tenant = ?
           AND billed_at IS NULL
           AND activity_type IN (
               'create_relay', 'update_relay', 'activate_relay',
               'deactivate_relay', 'autogenerate_invoice'
           )
         ORDER BY created_at ASC",
    ))
    .bind(tenant_pubkey)
    .fetch_all(pool())
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
