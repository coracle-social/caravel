use anyhow::Result;
use sqlx::{Sqlite, Transaction};

use crate::db::{pool, publish, with_tx};
use crate::models::{
    Activity, Bolt11, Invoice, InvoiceItem, RELAY_STATUS_ACTIVE, RELAY_STATUS_DELINQUENT,
    RELAY_STATUS_INACTIVE, Relay, Tenant,
};

// --- Activity ---

/// Stamp `billed_at` on activities that were reconciled without producing an
/// invoice (e.g. free-plan or not-yet-prorated changes), so a recovery pass
/// doesn't re-scan them.
pub async fn mark_activities_billed(activity_ids: &[String]) -> Result<()> {
    if activity_ids.is_empty() {
        return Ok(());
    }

    let now = chrono::Utc::now().timestamp();
    with_tx(async |tx| mark_activities_billed_tx(tx, activity_ids, now).await).await
}

/// Atomically record an `autogenerate_invoice` activity for the tenant, but only
/// if none has been recorded since `since` (the start of the current billing
/// period). Returns whether a new activity was inserted; `false` means the
/// period was already claimed.
///
/// The existence check and insert are a single statement, which SQLite runs
/// atomically, so concurrent pollers (or a restart racing the previous run)
/// can't both claim the same period. On success the activity is broadcast so the
/// billing consumer reconciles it like any other.
pub async fn try_autogenerate_invoice(tenant_pubkey: &str, since: i64) -> Result<bool> {
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().timestamp();

    let result = sqlx::query(
        "INSERT INTO activity (id, tenant, created_at, activity_type, resource_type, resource_id)
         SELECT ?, ?, ?, 'autogenerate_invoice', 'tenant', ?
         WHERE NOT EXISTS (
             SELECT 1 FROM activity
             WHERE tenant = ?
               AND activity_type = 'autogenerate_invoice'
               AND created_at >= ?
         )",
    )
    .bind(&id)
    .bind(tenant_pubkey)
    .bind(created_at)
    .bind(tenant_pubkey)
    .bind(tenant_pubkey)
    .bind(since)
    .execute(pool())
    .await?;

    if result.rows_affected() == 0 {
        return Ok(false);
    }

    publish(Activity {
        id,
        tenant: tenant_pubkey.to_string(),
        created_at,
        activity_type: "autogenerate_invoice".to_string(),
        resource_type: "tenant".to_string(),
        resource_id: tenant_pubkey.to_string(),
        billed_at: None,
    });

    Ok(true)
}

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
        insert_activity_tx(tx, "create_tenant", "tenant", &tenant.pubkey).await
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
        insert_activity_tx(tx, "update_tenant", "tenant", &tenant.pubkey).await
    })
    .await?;
    publish(activity);
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
        insert_activity_tx(tx, "create_relay", "relay", &relay.id).await
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
        insert_activity_tx(tx, "update_relay", "relay", &relay.id).await
    })
    .await?;
    publish(activity);
    Ok(())
}

pub async fn deactivate_relay(relay: &Relay) -> Result<()> {
    set_relay_status(&relay.id, RELAY_STATUS_INACTIVE, "deactivate_relay").await
}

#[allow(dead_code)] // wired up by the delinquency flow (not yet implemented)
pub async fn mark_relay_delinquent(relay: &Relay) -> Result<()> {
    set_relay_status(&relay.id, RELAY_STATUS_DELINQUENT, "mark_relay_delinquent").await
}

pub async fn activate_relay(relay: &Relay) -> Result<()> {
    set_relay_status(&relay.id, RELAY_STATUS_ACTIVE, "activate_relay").await
}

async fn set_relay_status(relay_id: &str, status: &str, activity_type: &str) -> Result<()> {
    let activity = with_tx(async |tx| {
        sqlx::query("UPDATE relay SET status = ?, synced = 0 WHERE id = ?")
            .bind(status)
            .bind(relay_id)
            .execute(&mut **tx)
            .await?;
        insert_activity_tx(tx, activity_type, "relay", relay_id).await
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
        insert_activity_tx(tx, "fail_relay_sync", "relay", &relay.id).await
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
        insert_activity_tx(tx, "complete_relay_sync", "relay", relay_id).await
    })
    .await?;
    publish(activity);
    Ok(())
}

// --- Invoices ---

/// Create an invoice with its line items, stamp `billed_at` on the activities
/// that produced them, and set the tenant's billing anchor when this is their
/// first invoice — all in one transaction. Returns the inserted invoice.
pub async fn create_invoice(
    invoice_id: &str,
    tenant_pubkey: &str,
    period_start: i64,
    period_end: i64,
    items: &[InvoiceItem],
    billed_activity_ids: &[String],
    new_billing_anchor: Option<i64>,
) -> Result<Invoice> {
    let now = chrono::Utc::now().timestamp();

    with_tx(async |tx| {
        let invoice =
            insert_invoice_tx(tx, invoice_id, tenant_pubkey, period_start, period_end).await?;

        for item in items {
            insert_invoice_item_tx(tx, item).await?;
        }

        mark_activities_billed_tx(tx, billed_activity_ids, now).await?;

        if let Some(anchor) = new_billing_anchor {
            set_tenant_billing_anchor_tx(tx, tenant_pubkey, anchor).await?;
        }

        Ok(invoice)
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
        insert_activity_tx(tx, "invoice_paid", "invoice", invoice_id).await
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
        "INSERT INTO activity (id, tenant, created_at, activity_type, resource_type, resource_id)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&tenant)
    .bind(created_at)
    .bind(activity_type)
    .bind(resource_type)
    .bind(resource_id)
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
    })
}

async fn insert_invoice_tx(
    tx: &mut Transaction<'_, Sqlite>,
    invoice_id: &str,
    tenant_pubkey: &str,
    period_start: i64,
    period_end: i64,
) -> Result<Invoice> {
    let now = chrono::Utc::now().timestamp();

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

async fn mark_activities_billed_tx(
    tx: &mut Transaction<'_, Sqlite>,
    activity_ids: &[String],
    billed_at: i64,
) -> Result<()> {
    for id in activity_ids {
        sqlx::query("UPDATE activity SET billed_at = ? WHERE id = ?")
            .bind(billed_at)
            .bind(id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

async fn set_tenant_billing_anchor_tx(
    tx: &mut Transaction<'_, Sqlite>,
    tenant_pubkey: &str,
    billing_anchor: i64,
) -> Result<()> {
    sqlx::query("UPDATE tenant SET billing_anchor = ? WHERE pubkey = ?")
        .bind(billing_anchor)
        .bind(tenant_pubkey)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
