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
            _ => anyhow::bail!("unknown resource_type: {}", resource_type),
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

    fn emit(&self, activity: Activity) {
        let _ = self.notify.send(activity);
    }

    pub async fn create_tenant(&self, tenant: &Tenant) -> Result<()> {
        if tenant.stripe_customer_id.trim().is_empty() {
            anyhow::bail!("stripe_customer_id is required");
        }

        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO tenant (pubkey, nwc_url, created_at, stripe_customer_id)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&tenant.pubkey)
        .bind(&tenant.nwc_url)
        .bind(tenant.created_at)
        .bind(&tenant.stripe_customer_id)
        .execute(&mut *tx)
        .await?;

        let activity =
            Self::insert_activity(&mut tx, "create_tenant", "tenant", &tenant.pubkey).await?;

        tx.commit().await?;
        self.emit(activity);
        Ok(())
    }

    pub async fn update_tenant(&self, tenant: &Tenant) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE tenant SET nwc_url = ? WHERE pubkey = ?")
            .bind(&tenant.nwc_url)
            .bind(&tenant.pubkey)
            .execute(&mut *tx)
            .await?;

        let activity =
            Self::insert_activity(&mut tx, "update_tenant", "tenant", &tenant.pubkey).await?;

        tx.commit().await?;
        self.emit(activity);
        Ok(())
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
            ) VALUES (?, ?, ?, ?, ?, 'active', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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

        let activity = Self::insert_activity(&mut tx, "create_relay", "relay", &relay.id).await?;

        tx.commit().await?;
        self.emit(activity);
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

        let activity = Self::insert_activity(&mut tx, "update_relay", "relay", &relay.id).await?;

        tx.commit().await?;
        self.emit(activity);
        Ok(())
    }

    pub async fn deactivate_relay(&self, relay: &Relay) -> Result<()> {
        self.set_relay_status(&relay.id, RELAY_STATUS_INACTIVE, "deactivate_relay")
            .await
    }

    pub async fn mark_relay_delinquent(&self, relay: &Relay) -> Result<()> {
        self.set_relay_status(&relay.id, RELAY_STATUS_DELINQUENT, "deactivate_relay")
            .await
    }

    async fn set_relay_status(
        &self,
        relay_id: &str,
        status: &str,
        activity_type: &str,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE relay SET status = ? WHERE id = ?")
            .bind(status)
            .bind(relay_id)
            .execute(&mut *tx)
            .await?;

        let activity = Self::insert_activity(&mut tx, activity_type, "relay", relay_id).await?;

        tx.commit().await?;
        self.emit(activity);
        Ok(())
    }

    pub async fn activate_relay(&self, relay: &Relay) -> Result<()> {
        self.set_relay_status(&relay.id, RELAY_STATUS_ACTIVE, "activate_relay")
            .await
    }

    pub async fn fail_relay_sync(&self, relay: &Relay, sync_error: String) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE relay SET sync_error = ? WHERE id = ?")
            .bind(&sync_error)
            .bind(&relay.id)
            .execute(&mut *tx)
            .await?;

        let activity =
            Self::insert_activity(&mut tx, "fail_relay_sync", "relay", &relay.id).await?;

        tx.commit().await?;
        self.emit(activity);
        Ok(())
    }

    pub async fn complete_relay_sync(&self, relay_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE relay SET synced = 1, sync_error = '' WHERE id = ?")
            .bind(relay_id)
            .execute(&mut *tx)
            .await?;

        let activity =
            Self::insert_activity(&mut tx, "complete_relay_sync", "relay", relay_id).await?;

        tx.commit().await?;
        self.emit(activity);
        Ok(())
    }

    pub async fn delete_relay_subscription_item(&self, relay_id: &str) -> Result<()> {
        sqlx::query("UPDATE relay SET stripe_subscription_item_id = NULL WHERE id = ?")
            .bind(relay_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_relay_subscription_item(
        &self,
        relay_id: &str,
        stripe_subscription_item_id: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE relay SET stripe_subscription_item_id = ? WHERE id = ?")
            .bind(stripe_subscription_item_id)
            .bind(relay_id)
            .execute(&self.pool)
            .await?;
        Ok(())
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    async fn test_pool() -> SqlitePool {
        let connect_options = SqliteConnectOptions::from_str("sqlite::memory:")
            .expect("valid sqlite memory url")
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(connect_options)
            .await
            .expect("connect sqlite memory db");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");

        pool
    }

    #[tokio::test]
    async fn create_tenant_rejects_empty_stripe_customer_id() {
        let pool = test_pool().await;
        let command = Command::new(pool);

        let tenant = Tenant {
            pubkey: "tenant_pubkey".to_string(),
            nwc_url: String::new(),
            nwc_error: None,
            created_at: 0,
            stripe_customer_id: "   ".to_string(),
            stripe_subscription_id: None,
            past_due_at: None,
        };

        let err = command
            .create_tenant(&tenant)
            .await
            .expect_err("empty customer id must be rejected");

        assert!(
            err.to_string().contains("stripe_customer_id is required"),
            "unexpected error: {err}"
        );
    }
}
