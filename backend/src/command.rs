use anyhow::Result;
use sqlx::{Sqlite, Transaction};

use crate::billing::BillingPeriod;
use crate::db::{pool, publish, with_tx};
use crate::models::{
    Activity, Bolt11, Invoice, InvoiceItem, RELAY_STATUS_ACTIVE, RELAY_STATUS_DELINQUENT,
    RELAY_STATUS_INACTIVE, Relay, Tenant,
};

// --- Tenants ---

pub async fn create_tenant(tenant: &Tenant) -> Result<()> {
    let activity = with_tx(async |tx| {
        sqlx::query(
            "INSERT INTO tenant (pubkey, nwc_url, created_at, stripe_customer_id)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&tenant.pubkey)
        .bind(&tenant.nwc_url)
        .bind(tenant.created_at)
        .bind(&tenant.stripe_customer_id)
        .execute(&mut **tx)
        .await?;
        insert_activity_tx(tx, "create_tenant", "tenant", &tenant.pubkey, None).await
    })
    .await?;
    publish(activity);
    Ok(())
}

pub async fn update_tenant(tenant: &Tenant) -> Result<()> {
    let activity = with_tx(async |tx| {
        sqlx::query("UPDATE tenant SET nwc_url = ? WHERE pubkey = ?")
            .bind(&tenant.nwc_url)
            .bind(&tenant.pubkey)
            .execute(&mut **tx)
            .await?;
        insert_activity_tx(tx, "update_tenant", "tenant", &tenant.pubkey, None).await
    })
    .await?;
    publish(activity);
    Ok(())
}

pub async fn set_tenant_billing_anchor(tenant: &Tenant) -> Result<()> {
    sqlx::query("UPDATE tenant SET billing_anchor = ? WHERE pubkey = ?")
        .bind(tenant.billing_anchor)
        .bind(&tenant.pubkey)
        .execute(pool())
        .await?;
    Ok(())
}

pub async fn clear_tenant_nwc_error(pubkey: &str) -> Result<()> {
    sqlx::query("UPDATE tenant SET nwc_error = NULL WHERE pubkey = ?")
        .bind(pubkey)
        .execute(pool())
        .await?;
    Ok(())
}

// --- Relays ---

pub async fn create_relay(relay: &Relay) -> Result<()> {
    let activity = with_tx(async |tx| {
        sqlx::query(
            "INSERT INTO relay (
                id, tenant, subdomain, plan, status, synced, sync_error,
                info_name, info_icon, info_description,
                policy_public_join, policy_strip_signatures,
                groups_enabled, management_enabled, blossom_enabled,
                livekit_enabled, push_enabled
            ) VALUES (?, ?, ?, ?, 'active', 0, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&relay.id)
        .bind(&relay.tenant)
        .bind(&relay.subdomain)
        .bind(&relay.plan)
        .bind(&relay.sync_error)
        .bind(&relay.info_name)
        .bind(&relay.info_icon)
        .bind(&relay.info_description)
        .bind(relay.policy_public_join)
        .bind(relay.policy_strip_signatures)
        .bind(relay.groups_enabled)
        .bind(relay.management_enabled)
        .bind(relay.blossom_enabled)
        .bind(relay.livekit_enabled)
        .bind(relay.push_enabled)
        .execute(&mut **tx)
        .await?;
        insert_activity_tx(tx, "create_relay", "relay", &relay.id, Some(&relay.plan)).await
    })
    .await?;
    publish(activity);
    Ok(())
}

pub async fn update_relay(relay: &Relay) -> Result<()> {
    let activity = with_tx(async |tx| {
        sqlx::query(
            "UPDATE relay
             SET tenant = ?, subdomain = ?, plan = ?, status = ?, sync_error = ?, synced = 0,
                 info_name = ?, info_icon = ?, info_description = ?,
                 policy_public_join = ?, policy_strip_signatures = ?,
                 groups_enabled = ?, management_enabled = ?, blossom_enabled = ?,
                 livekit_enabled = ?, push_enabled = ?
             WHERE id = ?",
        )
        .bind(&relay.tenant)
        .bind(&relay.subdomain)
        .bind(&relay.plan)
        .bind(&relay.status)
        .bind(&relay.sync_error)
        .bind(&relay.info_name)
        .bind(&relay.info_icon)
        .bind(&relay.info_description)
        .bind(relay.policy_public_join)
        .bind(relay.policy_strip_signatures)
        .bind(relay.groups_enabled)
        .bind(relay.management_enabled)
        .bind(relay.blossom_enabled)
        .bind(relay.livekit_enabled)
        .bind(relay.push_enabled)
        .bind(&relay.id)
        .execute(&mut **tx)
        .await?;
        insert_activity_tx(tx, "update_relay", "relay", &relay.id, Some(&relay.plan)).await
    })
    .await?;
    publish(activity);
    Ok(())
}

pub async fn activate_relay(relay: &Relay) -> Result<()> {
    set_relay_status(&relay.id, RELAY_STATUS_ACTIVE, "activate_relay").await
}

pub async fn deactivate_relay(relay: &Relay) -> Result<()> {
    set_relay_status(&relay.id, RELAY_STATUS_INACTIVE, "deactivate_relay").await
}

#[allow(dead_code)] // wired up by the delinquency flow (not yet implemented)
pub async fn mark_relay_delinquent(relay: &Relay) -> Result<()> {
    set_relay_status(&relay.id, RELAY_STATUS_DELINQUENT, "mark_relay_delinquent").await
}

async fn set_relay_status(relay_id: &str, status: &str, activity_type: &str) -> Result<()> {
    let activity = with_tx(async |tx| {
        sqlx::query("UPDATE relay SET status = ?, synced = 0 WHERE id = ?")
            .bind(status)
            .bind(relay_id)
            .execute(&mut **tx)
            .await?;
        insert_activity_tx(tx, activity_type, "relay", relay_id, None).await
    })
    .await?;
    publish(activity);
    Ok(())
}

pub async fn fail_relay_sync(relay: &Relay, sync_error: String) -> Result<()> {
    let activity = with_tx(async |tx| {
        sqlx::query("UPDATE relay SET synced = 0, sync_error = ? WHERE id = ?")
            .bind(&sync_error)
            .bind(&relay.id)
            .execute(&mut **tx)
            .await?;
        insert_activity_tx(tx, "fail_relay_sync", "relay", &relay.id, None).await
    })
    .await?;
    publish(activity);
    Ok(())
}

pub async fn complete_relay_sync(relay_id: &str) -> Result<()> {
    let activity = with_tx(async |tx| {
        sqlx::query("UPDATE relay SET synced = 1, sync_error = '' WHERE id = ?")
            .bind(relay_id)
            .execute(&mut **tx)
            .await?;
        insert_activity_tx(tx, "complete_relay_sync", "relay", relay_id, None).await
    })
    .await?;
    publish(activity);
    Ok(())
}

// --- Invoice items ---

/// Persist a reconciled activity's line item and mark the activity billed in one
/// transaction, so a recovery pass never re-bills it.
pub async fn insert_invoice_item_for_activity(invoice_item: &InvoiceItem, activity_id: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();

    with_tx(async |tx| {
        // Claim the activity first. If a concurrent reconcile pass already billed
        // it, the claim no-ops and we skip the item rather than duplicating it.
        if mark_activity_billed_tx(tx, activity_id, now).await? {
            insert_invoice_item_tx(tx, invoice_item).await?;
        }

        Ok(())
    })
    .await
}

/// Insert this period's renewal items and advance the tenant's `renewed_at`
/// marker to `period.start`, atomically and idempotently. Empty `items` is a
/// no-op — a tenant with no active paid relays has nothing to renew.
pub async fn insert_invoice_items_for_renewal(
    items: &[InvoiceItem],
    period: &BillingPeriod,
) -> Result<()> {
    let Some(first) = items.first() else {
        return Ok(());
    };
    let tenant_pubkey = &first.tenant_pubkey;

    with_tx(async |tx| {
        // Re-read the marker inside the transaction so the guard and the writes
        // commit together — this ensures idempotency so we don't double-invoice.
        let renewed_at = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT renewed_at FROM tenant WHERE pubkey = ?",
        )
        .bind(tenant_pubkey)
        .fetch_one(&mut **tx)
        .await?;

        if renewed_at.is_some_and(|at| at >= period.start) {
            return Ok(());
        }

        for item in items {
            insert_invoice_item_tx(tx, item).await?;
        }

        sqlx::query("UPDATE tenant SET renewed_at = ? WHERE pubkey = ?")
            .bind(period.start)
            .bind(tenant_pubkey)
            .execute(&mut **tx)
            .await?;

        Ok(())
    })
    .await
}

/// Mark an activity billed without a line item — for activities that produce no
/// charge (e.g. free-plan changes), so a recovery pass doesn't re-scan them.
pub async fn mark_activity_billed(activity_id: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();

    with_tx(async |tx| {
        mark_activity_billed_tx(tx, activity_id, now).await?;
        Ok(())
    })
    .await
}

// --- Invoices ---

/// Claim all of a tenant's outstanding items onto a new invoice. A non-positive
/// balance leaves the items outstanding so the credit carries to the next positive
/// invoice. Returns the invoice, or `None` when there's nothing to bill.
pub async fn create_invoice(
    tenant_pubkey: &str,
    period_start: i64,
    period_end: i64,
) -> Result<Option<Invoice>> {
    with_tx(async |tx| {
        let total = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(amount), 0) FROM invoice_item
             WHERE tenant_pubkey = ? AND invoice_id IS NULL",
        )
        .bind(tenant_pubkey)
        .fetch_one(&mut **tx)
        .await?;

        if total <= 0 {
            return Ok(None);
        }

        let invoice =
            insert_invoice_tx(tx, tenant_pubkey, period_start, period_end).await?;

        sqlx::query(
            "UPDATE invoice_item SET invoice_id = ?
             WHERE tenant_pubkey = ? AND invoice_id IS NULL",
        )
        .bind(&invoice.id)
        .bind(tenant_pubkey)
        .execute(&mut **tx)
        .await?;

        Ok(Some(invoice))
    })
    .await
}

pub async fn mark_invoice_paid(invoice_id: &str, method: &str) -> Result<()> {
    let updated_at = chrono::Utc::now().timestamp();

    let activity = with_tx(async |tx| {
        sqlx::query("UPDATE invoice SET status = 'paid', method = ?, updated_at = ? WHERE id = ?")
            .bind(method)
            .bind(updated_at)
            .bind(invoice_id)
            .execute(&mut **tx)
            .await?;
        insert_activity_tx(tx, "invoice_paid", "invoice", invoice_id, None).await
    })
    .await?;
    publish(activity);
    Ok(())
}

// --- Bolt11 records ---

pub async fn insert_bolt11(
    invoice_id: &str,
    lnbc: &str,
    msats: i64,
    expires_at: i64,
) -> Result<Option<Bolt11>> {
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().timestamp();

    Ok(sqlx::query_as::<_, Bolt11>(
        "INSERT INTO bolt11 (id, invoice_id, lnbc, msats, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(id)
    .bind(invoice_id)
    .bind(lnbc)
    .bind(msats)
    .bind(created_at)
    .bind(expires_at)
    .fetch_optional(pool())
    .await?)
}

pub async fn mark_bolt11_settled(bolt11_id: &str) -> Result<()> {
    let settled_at = chrono::Utc::now().timestamp();

    sqlx::query("UPDATE bolt11 SET settled_at = ? WHERE id = ?")
        .bind(settled_at)
        .bind(bolt11_id)
        .execute(pool())
        .await?;
    Ok(())
}

// --- Intents ---

/// Record the Stripe PaymentIntent that paid an invoice. Keyed by the Stripe
/// PaymentIntent id, so it's idempotent: a retried (idempotent) charge returns
/// the same id and the re-insert is a no-op rather than a primary-key conflict.
pub async fn insert_intent(intent_id: &str, invoice_id: &str) -> Result<()> {
    let created_at = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO intent (id, invoice_id, created_at)
         VALUES (?, ?, ?) ON CONFLICT(id) DO NOTHING",
    )
    .bind(intent_id)
    .bind(invoice_id)
    .bind(created_at)
    .execute(pool())
    .await?;
    Ok(())
}

// --- Internal utils that take an explicit transaction ---

async fn insert_activity_tx(
    tx: &mut Transaction<'_, Sqlite>,
    activity_type: &str,
    resource_type: &str,
    resource_id: &str,
    plan_id: Option<&str>,
) -> Result<Activity> {
    let tenant = match resource_type {
        "tenant" => resource_id.to_string(),
        "relay" => {
            sqlx::query_scalar::<_, String>("SELECT tenant FROM relay WHERE id = ?")
                .bind(resource_id)
                .fetch_one(&mut **tx)
                .await?
        }
        _ => anyhow::bail!("unknown resource_type: {resource_type}"),
    };

    let id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO activity (id, tenant, created_at, activity_type, resource_type, resource_id, plan_id)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&tenant)
    .bind(created_at)
    .bind(activity_type)
    .bind(resource_type)
    .bind(resource_id)
    .bind(plan_id)
    .execute(&mut **tx)
    .await?;

    Ok(Activity {
        id,
        tenant,
        created_at,
        activity_type: activity_type.to_string(),
        resource_type: resource_type.to_string(),
        resource_id: resource_id.to_string(),
        billed_at: None,
        plan_id: plan_id.map(str::to_string),
    })
}

async fn insert_invoice_tx(
    tx: &mut Transaction<'_, Sqlite>,
    tenant_pubkey: &str,
    period_start: i64,
    period_end: i64,
) -> Result<Invoice> {
    let now = chrono::Utc::now().timestamp();
    let invoice_id = uuid::Uuid::new_v4().to_string();

    Ok(sqlx::query_as::<_, Invoice>(
        "INSERT INTO invoice (id, tenant_pubkey, status, period_start, period_end, created_at, updated_at)
         VALUES (?, ?, 'open', ?, ?, ?, ?) RETURNING *",
    )
    .bind(invoice_id)
    .bind(tenant_pubkey)
    .bind(period_start)
    .bind(period_end)
    .bind(now)
    .bind(now)
    .fetch_one(&mut **tx)
    .await?)
}

async fn insert_invoice_item_tx(tx: &mut Transaction<'_, Sqlite>, item: &InvoiceItem) -> Result<()> {
    sqlx::query(
        "INSERT INTO invoice_item
            (id, invoice_id, activity_id, tenant_pubkey, relay_id, plan, amount, description, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&item.id)
    .bind(&item.invoice_id)
    .bind(&item.activity_id)
    .bind(&item.tenant_pubkey)
    .bind(&item.relay_id)
    .bind(&item.plan)
    .bind(item.amount)
    .bind(&item.description)
    .bind(item.created_at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Claim an activity as billed. Returns `true` if this call set the marker, and
/// `false` if it was already set — e.g. a concurrent reconcile pass won the race —
/// so callers can skip work that would otherwise double-bill.
async fn mark_activity_billed_tx(
    tx: &mut Transaction<'_, Sqlite>,
    activity_id: &str,
    billed_at: i64,
) -> Result<bool> {
    let result = sqlx::query("UPDATE activity SET billed_at = ? WHERE id = ? AND billed_at IS NULL")
        .bind(billed_at)
        .bind(activity_id)
        .execute(&mut **tx)
        .await?;
    Ok(result.rows_affected() > 0)
}
