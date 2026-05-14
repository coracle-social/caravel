use anyhow::Result;
use sqlx::SqlitePool;

use crate::env::Env;
use crate::models::{Activity, Plan, Relay, Tenant};

#[derive(Clone)]
pub struct Query {
    pool: SqlitePool,
    env: Env,
}

impl Query {
    pub fn new(pool: SqlitePool, env: &Env) -> Self {
        Self {
            pool,
            env: env.clone(),
        }
    }

    pub async fn list_tenants(&self) -> Result<Vec<Tenant>> {
        let rows = sqlx::query_as::<_, Tenant>(
            "SELECT pubkey, nwc_url, nwc_error, created_at, stripe_customer_id, stripe_subscription_id, past_due_at
             FROM tenant
             ORDER BY pubkey",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_tenant(&self, pubkey: &str) -> Result<Option<Tenant>> {
        let row = sqlx::query_as::<_, Tenant>(
            "SELECT pubkey, nwc_url, nwc_error, created_at, stripe_customer_id, stripe_subscription_id, past_due_at
             FROM tenant
             WHERE pubkey = ?",
        )
        .bind(pubkey)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub fn list_plans(&self) -> Vec<Plan> {
        vec![
            Plan {
                id: "free".to_string(),
                name: "Free".to_string(),
                amount: 0,
                members: Some(10),
                blossom: false,
                livekit: false,
                stripe_price_id: None,
            },
            Plan {
                id: "basic".to_string(),
                name: "Basic".to_string(),
                amount: 500,
                members: Some(100),
                blossom: true,
                livekit: true,
                stripe_price_id: Some(self.env.stripe_price_basic.clone()),
            },
            Plan {
                id: "growth".to_string(),
                name: "Growth".to_string(),
                amount: 2500,
                members: None,
                blossom: true,
                livekit: true,
                stripe_price_id: Some(self.env.stripe_price_growth.clone()),
            },
        ]
    }

    pub fn get_plan(&self, plan_id: &str) -> Option<Plan> {
        self.list_plans().into_iter().find(|p| p.id == plan_id)
    }

    /// True for any plan that costs money. Doesn't require an instance because
    /// the answer doesn't depend on Stripe price ids — only the canonical plan id.
    pub fn is_paid_plan(plan_id: &str) -> bool {
        matches!(plan_id, "basic" | "growth")
    }

    pub async fn list_relays(&self) -> Result<Vec<Relay>> {
        let rows = sqlx::query_as::<_, Relay>(
            "SELECT id, tenant, schema, subdomain, plan, stripe_subscription_item_id,
                     status, sync_error,
                     info_name, info_icon, info_description,
                     policy_public_join, policy_strip_signatures,
                     groups_enabled, management_enabled, blossom_enabled,
                     livekit_enabled, push_enabled, synced
              FROM relay
              ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn list_relays_pending_sync(&self) -> Result<Vec<Relay>> {
        let rows = sqlx::query_as::<_, Relay>(
            "SELECT id, tenant, schema, subdomain, plan, stripe_subscription_item_id,
                     status, sync_error,
                     info_name, info_icon, info_description,
                     policy_public_join, policy_strip_signatures,
                     groups_enabled, management_enabled, blossom_enabled,
                     livekit_enabled, push_enabled, synced
              FROM relay
              WHERE synced = 0 OR TRIM(sync_error) != ''
              ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn list_relays_for_tenant(&self, tenant_id: &str) -> Result<Vec<Relay>> {
        let rows = sqlx::query_as::<_, Relay>(
            "SELECT id, tenant, schema, subdomain, plan, stripe_subscription_item_id,
                     status, sync_error,
                     info_name, info_icon, info_description,
                     policy_public_join, policy_strip_signatures,
                     groups_enabled, management_enabled, blossom_enabled,
                     livekit_enabled, push_enabled, synced
              FROM relay
              WHERE tenant = ?
              ORDER BY id",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_relay(&self, id: &str) -> Result<Option<Relay>> {
        let row = sqlx::query_as::<_, Relay>(
            "SELECT id, tenant, schema, subdomain, plan, stripe_subscription_item_id,
                     status, sync_error,
                     info_name, info_icon, info_description,
                     policy_public_join, policy_strip_signatures,
                     groups_enabled, management_enabled, blossom_enabled,
                     livekit_enabled, push_enabled, synced
              FROM relay
              WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn get_tenant_by_stripe_customer_id(
        &self,
        stripe_customer_id: &str,
    ) -> Result<Option<Tenant>> {
        let row = sqlx::query_as::<_, Tenant>(
            "SELECT pubkey, nwc_url, nwc_error, created_at, stripe_customer_id, stripe_subscription_id, past_due_at
             FROM tenant
             WHERE stripe_customer_id = ?",
        )
        .bind(stripe_customer_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn get_invoice_nwc_payment_state(&self, invoice_id: &str) -> Result<Option<String>> {
        let state = sqlx::query_scalar::<_, String>(
            "SELECT state FROM invoice_nwc_payment WHERE invoice_id = ?",
        )
        .bind(invoice_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(state)
    }

    pub async fn get_invoice_manual_lightning_bolt11(
        &self,
        invoice_id: &str,
    ) -> Result<Option<String>> {
        let bolt11 = sqlx::query_scalar::<_, String>(
            "SELECT bolt11 FROM invoice_manual_lightning_payment WHERE invoice_id = ?",
        )
        .bind(invoice_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(bolt11)
    }

    pub async fn list_activity_for_relay(&self, relay_id: &str) -> Result<Vec<Activity>> {
        let rows = sqlx::query_as::<_, Activity>(
            "SELECT id, tenant, created_at, activity_type, resource_type, resource_id
             FROM activity
             WHERE resource_type = 'relay' AND resource_id = ?
             ORDER BY created_at DESC, id DESC",
        )
        .bind(relay_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn relay_has_completed_sync(&self, relay_id: &str) -> Result<bool> {
        let found = sqlx::query_scalar::<_, i64>(
            "SELECT 1
             FROM activity
             WHERE resource_type = 'relay'
               AND resource_id = ?
               AND activity_type = 'complete_relay_sync'
             LIMIT 1",
        )
        .bind(relay_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(found.is_some())
    }
}
