use std::path::Path;

use anyhow::Result;
use sqlx::{Sqlite, SqlitePool, Transaction, sqlite::SqlitePoolOptions};

use crate::models::{Activity, Invoice, InvoiceItem, Relay, Tenant};

#[derive(Clone)]
pub struct Repo {
    pub pool: SqlitePool,
}

impl Repo {
    pub async fn new() -> Result<Self> {
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://data/caravel.db".to_string());

        if let Some(path) = database_url.strip_prefix("sqlite://")
            && !path.is_empty()
            && path != ":memory:"
            && let Some(parent) = Path::new(path).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await?;

        sqlx::query("PRAGMA journal_mode = WAL;")
            .execute(&pool)
            .await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Self { pool })
    }

    async fn insert_activity(
        tx: &mut Transaction<'_, Sqlite>,
        activity_type: &str,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<()> {
        let tenant = match resource_type {
            "tenant" => resource_id.to_string(),
            "relay" => {
                sqlx::query_scalar::<_, String>("SELECT tenant FROM relay WHERE id = ?")
                    .bind(resource_id)
                    .fetch_one(&mut **tx)
                    .await?
            }
            "invoice" => {
                sqlx::query_scalar::<_, String>("SELECT tenant FROM invoice WHERE id = ?")
                    .bind(resource_id)
                    .fetch_one(&mut **tx)
                    .await?
            }
            _ => anyhow::bail!("unknown resource_type: {}", resource_type),
        };

        sqlx::query(
            "INSERT INTO activity (id, tenant, created_at, activity_type, resource_type, resource_id)
             VALUES (?, ?, strftime('%s','now'), ?, ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(tenant)
        .bind(activity_type)
        .bind(resource_type)
        .bind(resource_id)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub async fn list_tenants(&self) -> Result<Vec<Tenant>> {
        let rows = sqlx::query_as::<_, Tenant>(
            "SELECT pubkey, nwc_url, created_at, billing_anchor
             FROM tenant
             ORDER BY pubkey",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_tenant(&self, pubkey: &str) -> Result<Option<Tenant>> {
        let row = sqlx::query_as::<_, Tenant>(
            "SELECT pubkey, nwc_url, created_at, billing_anchor
             FROM tenant
             WHERE pubkey = ?",
        )
        .bind(pubkey)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn create_tenant(&self, tenant: &Tenant) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO tenant (pubkey, nwc_url, created_at, billing_anchor)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&tenant.pubkey)
        .bind(&tenant.nwc_url)
        .bind(tenant.created_at)
        .bind(tenant.billing_anchor)
        .execute(&mut *tx)
        .await?;

        Self::insert_activity(&mut tx, "create_tenant", "tenant", &tenant.pubkey).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn update_tenant_billing_anchor(&self, pubkey: &str, billing_anchor: i64) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE tenant SET billing_anchor = ? WHERE pubkey = ?")
            .bind(billing_anchor)
            .bind(pubkey)
            .execute(&mut *tx)
            .await?;

        Self::insert_activity(&mut tx, "update_tenant_billing_anchor", "tenant", pubkey).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn update_tenant_nwc_url(&self, pubkey: &str, nwc_url: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE tenant SET nwc_url = ? WHERE pubkey = ?")
            .bind(nwc_url)
            .bind(pubkey)
            .execute(&mut *tx)
            .await?;

        Self::insert_activity(&mut tx, "update_tenant_nwc_url", "tenant", pubkey).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn list_relays(&self, tenant_id: Option<&str>) -> Result<Vec<Relay>> {
        let rows = if let Some(tenant) = tenant_id {
            sqlx::query_as::<_, Relay>(
                "SELECT id, tenant, schema, subdomain, plan, status, sync_error,
                        info_name, info_icon, info_description,
                        policy_public_join, policy_strip_signatures,
                        groups_enabled, management_enabled, blossom_enabled,
                        livekit_enabled, push_enabled
                 FROM relay
                 WHERE tenant = ?
                 ORDER BY id",
            )
            .bind(tenant)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, Relay>(
                "SELECT id, tenant, schema, subdomain, plan, status, sync_error,
                        info_name, info_icon, info_description,
                        policy_public_join, policy_strip_signatures,
                        groups_enabled, management_enabled, blossom_enabled,
                        livekit_enabled, push_enabled
                 FROM relay
                 ORDER BY id",
            )
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows)
    }

    pub async fn get_relay(&self, id: &str) -> Result<Option<Relay>> {
        let row = sqlx::query_as::<_, Relay>(
            "SELECT id, tenant, schema, subdomain, plan, status, sync_error,
                    info_name, info_icon, info_description,
                    policy_public_join, policy_strip_signatures,
                    groups_enabled, management_enabled, blossom_enabled,
                    livekit_enabled, push_enabled
             FROM relay
             WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn create_relay(&self, relay: &Relay) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO relay (
                id, tenant, schema, subdomain, plan, status, sync_error,
                info_name, info_icon, info_description,
                policy_public_join, policy_strip_signatures,
                groups_enabled, management_enabled, blossom_enabled,
                livekit_enabled, push_enabled
            ) VALUES (?, ?, ?, ?, ?, 'new', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&relay.id)
        .bind(&relay.tenant)
        .bind(&relay.schema)
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
        .execute(&mut *tx)
        .await?;

        Self::insert_activity(&mut tx, "create_relay", "relay", &relay.id).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn update_relay(&self, relay: &Relay) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "UPDATE relay
             SET tenant = ?, schema = ?, subdomain = ?, plan = ?, status = ?, sync_error = ?,
                 info_name = ?, info_icon = ?, info_description = ?,
                 policy_public_join = ?, policy_strip_signatures = ?,
                 groups_enabled = ?, management_enabled = ?, blossom_enabled = ?,
                 livekit_enabled = ?, push_enabled = ?
             WHERE id = ?",
        )
        .bind(&relay.tenant)
        .bind(&relay.schema)
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
        .execute(&mut *tx)
        .await?;

        Self::insert_activity(&mut tx, "update_relay", "relay", &relay.id).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn deactivate_relay(&self, relay: &Relay) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE relay SET status = 'inactive' WHERE id = ?")
            .bind(&relay.id)
            .execute(&mut *tx)
            .await?;

        Self::insert_activity(&mut tx, "deactivate_relay", "relay", &relay.id).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn activate_relay(&self, relay: &Relay) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE relay SET status = 'active' WHERE id = ?")
            .bind(&relay.id)
            .execute(&mut *tx)
            .await?;

        Self::insert_activity(&mut tx, "activate_relay", "relay", &relay.id).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn fail_relay_sync(&self, relay: &Relay, sync_error: String) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE relay SET status = 'inactive', sync_error = ? WHERE id = ?")
            .bind(&sync_error)
            .bind(&relay.id)
            .execute(&mut *tx)
            .await?;

        Self::insert_activity(&mut tx, "fail_relay_sync", "relay", &relay.id).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn create_invoice(&self, invoice: &Invoice, invoice_items: &[InvoiceItem]) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO invoice (
                id, tenant, status, created_at, attempted_at, error, closed_at,
                sent_at, paid_at, bolt11, period_start, period_end
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&invoice.id)
        .bind(&invoice.tenant)
        .bind(&invoice.status)
        .bind(invoice.created_at)
        .bind(invoice.attempted_at)
        .bind(&invoice.error)
        .bind(invoice.closed_at)
        .bind(invoice.sent_at)
        .bind(invoice.paid_at)
        .bind(&invoice.bolt11)
        .bind(invoice.period_start)
        .bind(invoice.period_end)
        .execute(&mut *tx)
        .await?;

        for item in invoice_items {
            sqlx::query("INSERT INTO invoice_item (id, invoice, relay, sats) VALUES (?, ?, ?, ?)")
                .bind(&item.id)
                .bind(&item.invoice)
                .bind(&item.relay)
                .bind(item.sats)
                .execute(&mut *tx)
                .await?;
        }

        Self::insert_activity(&mut tx, "create_invoice", "invoice", &invoice.id).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn list_invoices(&self, tenant_id: Option<&str>) -> Result<Vec<Invoice>> {
        let rows = if let Some(tenant) = tenant_id {
            sqlx::query_as::<_, Invoice>(
                "SELECT id, tenant, status, created_at, attempted_at, error, closed_at,
                        sent_at, paid_at, bolt11, period_start, period_end
                 FROM invoice
                 WHERE tenant = ?
                 ORDER BY created_at DESC",
            )
            .bind(tenant)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, Invoice>(
                "SELECT id, tenant, status, created_at, attempted_at, error, closed_at,
                        sent_at, paid_at, bolt11, period_start, period_end
                 FROM invoice
                 ORDER BY created_at DESC",
            )
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows)
    }

    pub async fn mark_invoice_paid(&self, invoice_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "UPDATE invoice
             SET status = 'paid', paid_at = strftime('%s','now'), error = ''
             WHERE id = ?",
        )
        .bind(invoice_id)
        .execute(&mut *tx)
        .await?;

        Self::insert_activity(&mut tx, "mark_invoice_paid", "invoice", invoice_id).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_invoice_attempted(&self, invoice_id: &str, error: Option<&str>) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "UPDATE invoice
             SET attempted_at = strftime('%s','now'), error = COALESCE(?, error)
             WHERE id = ?",
        )
        .bind(error)
        .bind(invoice_id)
        .execute(&mut *tx)
        .await?;

        Self::insert_activity(&mut tx, "mark_invoice_attempted", "invoice", invoice_id).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_invoice_sent(&self, invoice_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE invoice SET sent_at = strftime('%s','now') WHERE id = ?")
            .bind(invoice_id)
            .execute(&mut *tx)
            .await?;

        Self::insert_activity(&mut tx, "mark_invoice_sent", "invoice", invoice_id).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_invoice_closed(&self, invoice_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "UPDATE invoice
             SET status = 'closed', closed_at = strftime('%s','now')
             WHERE id = ?",
        )
        .bind(invoice_id)
        .execute(&mut *tx)
        .await?;

        Self::insert_activity(&mut tx, "mark_invoice_closed", "invoice", invoice_id).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn list_activity(&self, since: &i64, tenant: Option<&str>) -> Result<Vec<Activity>> {
        let rows = if let Some(tenant_pubkey) = tenant {
            sqlx::query_as::<_, Activity>(
                "SELECT id, tenant, created_at, activity_type, resource_type, resource_id
                 FROM activity
                 WHERE created_at > ? AND tenant = ?
                 ORDER BY created_at, id",
            )
            .bind(since)
            .bind(tenant_pubkey)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, Activity>(
                "SELECT id, tenant, created_at, activity_type, resource_type, resource_id
                 FROM activity
                 WHERE created_at > ?
                 ORDER BY created_at, id",
            )
            .bind(since)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows)
    }

    pub async fn get_invoice_items(&self, invoice_id: &str) -> Result<Vec<InvoiceItem>> {
        let rows = sqlx::query_as::<_, InvoiceItem>(
            "SELECT id, invoice, relay, sats
             FROM invoice_item
             WHERE invoice = ?",
        )
        .bind(invoice_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn total_active_paid_relays_for_tenant(&self, tenant: &str) -> Result<i64> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM relay
             WHERE tenant = ? AND status = 'active' AND plan != 'free'",
        )
        .bind(tenant)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    pub async fn total_pending_invoices_for_tenant(&self, tenant: &str) -> Result<i64> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM invoice
             WHERE tenant = ? AND status = 'pending'",
        )
        .bind(tenant)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    pub async fn get_relay_plan_amount_sats(&self, plan: &str) -> Result<i64> {
        let sats = match plan {
            "free" => 0,
            "basic" => 10_000,
            "growth" => 50_000,
            _ => 0,
        };
        Ok(sats)
    }

}
