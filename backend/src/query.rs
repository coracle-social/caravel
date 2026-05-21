use anyhow::Result;
use sqlx::SqlitePool;

use crate::env::Env;
use crate::models::{Activity, LightningInvoice, Plan, Relay, Tenant};

fn select_tenant(tail: &str) -> String {
    format!("SELECT * FROM tenant {tail}")
}

fn select_relay(tail: &str) -> String {
    format!("SELECT * FROM relay {tail}")
}

fn select_activity(tail: &str) -> String {
    format!("SELECT * FROM activity {tail}")
}

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

    // Plans

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

    pub fn is_paid_plan(&self, plan_id: &str) -> bool {
        self.get_plan(plan_id).is_some_and(|p| p.amount > 0)
    }

    // Tenants

    pub async fn list_tenants(&self) -> Result<Vec<Tenant>> {
        let rows = sqlx::query_as::<_, Tenant>(&select_tenant(""))
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    pub async fn get_tenant(&self, pubkey: &str) -> Result<Option<Tenant>> {
        let row = sqlx::query_as::<_, Tenant>(&select_tenant("WHERE pubkey = ?"))
            .bind(pubkey)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    pub async fn get_tenant_by_stripe_customer_id(
        &self,
        stripe_customer_id: &str,
    ) -> Result<Option<Tenant>> {
        let row = sqlx::query_as::<_, Tenant>(&select_tenant("WHERE stripe_customer_id = ?"))
            .bind(stripe_customer_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    // Relays

    pub async fn list_relays(&self) -> Result<Vec<Relay>> {
        let rows = sqlx::query_as::<_, Relay>(&select_relay(""))
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    pub async fn list_relays_pending_sync(&self) -> Result<Vec<Relay>> {
        let rows = sqlx::query_as::<_, Relay>(&select_relay(
            "WHERE synced = 0 OR TRIM(sync_error) != ''",
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn list_relays_for_tenant(&self, tenant_id: &str) -> Result<Vec<Relay>> {
        let rows = sqlx::query_as::<_, Relay>(&select_relay("WHERE tenant = ?"))
            .bind(tenant_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    pub async fn get_relay(&self, id: &str) -> Result<Option<Relay>> {
        let row = sqlx::query_as::<_, Relay>(&select_relay("WHERE id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    // Invoices

    pub async fn get_lightning_invoice(
        &self,
        stripe_invoice_id: &str,
    ) -> Result<Option<LightningInvoice>> {
        let row = sqlx::query_as::<_, LightningInvoice>(
            "SELECT * FROM lightning_invoice WHERE stripe_invoice_id = ?",
        )
        .bind(stripe_invoice_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    // Activity

    pub async fn list_activity_for_resource(&self, resource_id: &str) -> Result<Vec<Activity>> {
        let rows = sqlx::query_as::<_, Activity>(&select_activity("WHERE resource_id = ? ORDER BY created_at DESC"))
        .bind(resource_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_latest_activity_for_resource_and_type(
        &self,
        resource_id: &str,
        activity_type: &str,
    ) -> Result<Option<Activity>> {
        let row = sqlx::query_as::<_, Activity>(&select_activity(
            "WHERE resource_id = ? AND activity_type = ? ORDER BY created_at DESC LIMIT 1",
        ))
        .bind(resource_id)
        .bind(activity_type)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }
}
