use anyhow::Result;
use sqlx::{Sqlite, SqlitePool, Transaction};
use tokio::sync::broadcast;

use crate::models::{
    Activity, RELAY_STATUS_ACTIVE, RELAY_STATUS_DELINQUENT, RELAY_STATUS_INACTIVE, Relay, Tenant,
};

#[derive(Clone)]
pub struct Command {
    pool: SqlitePool,
    pub notify: broadcast::Sender<Activity>,
}

impl Command {
    pub fn new(pool: SqlitePool) -> Self {
        let (notify, _) = broadcast::channel(64);
        Self { pool, notify }
    }

    // Activity

    async fn insert_activity(
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
        })
    }

    /// Run `f` inside a transaction, record an activity row, commit, and broadcast.
    async fn with_activity<F>(
        &self,
        activity_type: &str,
        resource_type: &str,
        resource_id: &str,
        f: F,
    ) -> Result<()>
    where
        F: AsyncFnOnce(&mut Transaction<'_, Sqlite>) -> Result<()>,
    {
        let mut tx = self.pool.begin().await?;
        f(&mut tx).await?;
        let activity =
            Self::insert_activity(&mut tx, activity_type, resource_type, resource_id).await?;
        tx.commit().await?;
        let _ = self.notify.send(activity);
        Ok(())
    }

    // Tenants

    pub async fn create_tenant(&self, tenant: &Tenant) -> Result<()> {
        self.with_activity("create_tenant", "tenant", &tenant.pubkey, async |tx| {
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
            Ok(())
        })
        .await
    }

    pub async fn update_tenant(&self, tenant: &Tenant) -> Result<()> {
        self.with_activity("update_tenant", "tenant", &tenant.pubkey, async |tx| {
            sqlx::query("UPDATE tenant SET nwc_url = ? WHERE pubkey = ?")
                .bind(&tenant.nwc_url)
                .bind(&tenant.pubkey)
                .execute(&mut **tx)
                .await?;
            Ok(())
        })
        .await
    }

    pub async fn set_tenant_subscription(
        &self,
        pubkey: &str,
        stripe_subscription_id: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE tenant SET stripe_subscription_id = ? WHERE pubkey = ?")
            .bind(stripe_subscription_id)
            .bind(pubkey)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn clear_tenant_subscription(&self, pubkey: &str) -> Result<()> {
        sqlx::query("UPDATE tenant SET stripe_subscription_id = NULL WHERE pubkey = ?")
            .bind(pubkey)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_tenant_nwc_error(&self, pubkey: &str, error: &str) -> Result<()> {
        sqlx::query("UPDATE tenant SET nwc_error = ? WHERE pubkey = ?")
            .bind(error)
            .bind(pubkey)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn clear_tenant_nwc_error(&self, pubkey: &str) -> Result<()> {
        sqlx::query("UPDATE tenant SET nwc_error = NULL WHERE pubkey = ?")
            .bind(pubkey)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_tenant_past_due(&self, pubkey: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE tenant SET past_due_at = ? WHERE pubkey = ?")
            .bind(now)
            .bind(pubkey)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn clear_tenant_past_due(&self, pubkey: &str) -> Result<()> {
        sqlx::query("UPDATE tenant SET past_due_at = NULL WHERE pubkey = ?")
            .bind(pubkey)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // Relays

    pub async fn create_relay(&self, relay: &Relay) -> Result<()> {
        self.with_activity("create_relay", "relay", &relay.id, async |tx| {
            sqlx::query(
                "INSERT INTO relay (
                    id, tenant, schema, subdomain, plan, status, synced, sync_error,
                    info_name, info_icon, info_description,
                    policy_public_join, policy_strip_signatures,
                    groups_enabled, management_enabled, blossom_enabled,
                    livekit_enabled, push_enabled
                ) VALUES (?, ?, ?, ?, ?, 'active', 0, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
            .execute(&mut **tx)
            .await?;
            Ok(())
        })
        .await
    }

    pub async fn update_relay(&self, relay: &Relay) -> Result<()> {
        self.with_activity("update_relay", "relay", &relay.id, async |tx| {
            sqlx::query(
                "UPDATE relay
                 SET tenant = ?, schema = ?, subdomain = ?, plan = ?, status = ?, sync_error = ?, synced = 0,
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
            .execute(&mut **tx)
            .await?;
            Ok(())
        })
        .await
    }

    pub async fn deactivate_relay(&self, relay: &Relay) -> Result<()> {
        self.set_relay_status(&relay.id, RELAY_STATUS_INACTIVE, "deactivate_relay")
            .await
    }

    pub async fn mark_relay_delinquent(&self, relay: &Relay) -> Result<()> {
        self.set_relay_status(&relay.id, RELAY_STATUS_DELINQUENT, "mark_relay_delinquent")
            .await
    }

    pub async fn activate_relay(&self, relay: &Relay) -> Result<()> {
        self.set_relay_status(&relay.id, RELAY_STATUS_ACTIVE, "activate_relay")
            .await
    }

    async fn set_relay_status(
        &self,
        relay_id: &str,
        status: &str,
        activity_type: &str,
    ) -> Result<()> {
        self.with_activity(activity_type, "relay", relay_id, async |tx| {
            sqlx::query("UPDATE relay SET status = ?, synced = 0 WHERE id = ?")
                .bind(status)
                .bind(relay_id)
                .execute(&mut **tx)
                .await?;
            Ok(())
        })
        .await
    }

    pub async fn fail_relay_sync(&self, relay: &Relay, sync_error: String) -> Result<()> {
        self.with_activity("fail_relay_sync", "relay", &relay.id, async |tx| {
            sqlx::query("UPDATE relay SET synced = 0, sync_error = ? WHERE id = ?")
                .bind(&sync_error)
                .bind(&relay.id)
                .execute(&mut **tx)
                .await?;
            Ok(())
        })
        .await
    }

    pub async fn complete_relay_sync(&self, relay_id: &str) -> Result<()> {
        self.with_activity("complete_relay_sync", "relay", relay_id, async |tx| {
            sqlx::query("UPDATE relay SET synced = 1, sync_error = '' WHERE id = ?")
                .bind(relay_id)
                .execute(&mut **tx)
                .await?;
            Ok(())
        })
        .await
    }

    // Invoices

    /// Insert a freshly minted pending bolt11 for an invoice. Returns `false` if
    /// a row already exists (lost an insert race), in which case the caller
    /// should read and use the existing row's bolt11.
    pub async fn insert_pending_lightning_invoice(
        &self,
        stripe_invoice_id: &str,
        tenant_pubkey: &str,
        bolt11: &str,
        expires_at: i64,
    ) -> Result<bool> {
        let now = chrono::Utc::now().timestamp();
        let result = sqlx::query(
            "INSERT INTO lightning_invoice
             (stripe_invoice_id, tenant_pubkey, bolt11, status, expires_at, created_at, updated_at)
             VALUES (?, ?, ?, 'pending', ?, ?, ?)
             ON CONFLICT(stripe_invoice_id) DO NOTHING",
        )
        .bind(stripe_invoice_id)
        .bind(tenant_pubkey)
        .bind(bolt11)
        .bind(expires_at)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Replace the stored bolt11 for a still-pending invoice whose previous
    /// invoice expired. No-op once the invoice is paid, so this can never
    /// overwrite a settled invoice.
    pub async fn regenerate_lightning_invoice(
        &self,
        stripe_invoice_id: &str,
        bolt11: &str,
        expires_at: i64,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "UPDATE lightning_invoice
             SET bolt11 = ?, expires_at = ?, updated_at = ?
             WHERE stripe_invoice_id = ? AND status = 'pending'",
        )
        .bind(bolt11)
        .bind(expires_at)
        .bind(now)
        .bind(stripe_invoice_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Mark a pending invoice paid, recording which method settled it. The
    /// `status = 'pending'` guard makes this idempotent and first-writer-wins:
    /// a later reconcile won't clobber the method recorded by whoever settled
    /// it first.
    pub async fn mark_lightning_invoice_paid(&self, stripe_invoice_id: &str, method: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "UPDATE lightning_invoice
             SET status = 'paid', paid_method = ?, updated_at = ?
             WHERE stripe_invoice_id = ? AND status = 'pending'",
        )
        .bind(method)
        .bind(now)
        .bind(stripe_invoice_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
