use anyhow::Result;
use sqlx::SqlitePool;

use crate::models::{Activity, Plan, Relay, Tenant};

#[derive(Clone)]
pub struct Query {
    pool: SqlitePool,
}

impl Query {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list_tenants(&self) -> Result<Vec<Tenant>> {
        let rows = sqlx::query_as::<_, Tenant>(
            "SELECT pubkey, nwc_url, created_at
             FROM tenant
             ORDER BY pubkey",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_tenant(&self, pubkey: &str) -> Result<Option<Tenant>> {
        let row = sqlx::query_as::<_, Tenant>(
            "SELECT pubkey, nwc_url, created_at
             FROM tenant
             WHERE pubkey = ?",
        )
        .bind(pubkey)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub fn list_plans() -> Vec<Plan> {
        vec![
            Plan {
                id: "free".to_string(),
                name: "Free".to_string(),
                sats: 0,
                members: Some(10),
                blossom: false,
                livekit: false,
            },
            Plan {
                id: "basic".to_string(),
                name: "Basic".to_string(),
                sats: 10_000,
                members: Some(100),
                blossom: true,
                livekit: true,
            },
            Plan {
                id: "growth".to_string(),
                name: "Growth".to_string(),
                sats: 50_000,
                members: None,
                blossom: true,
                livekit: true,
            },
        ]
    }

    pub async fn list_relays(&self) -> Result<Vec<Relay>> {
        let rows = sqlx::query_as::<_, Relay>(
             "SELECT id, tenant, schema, subdomain, plan, status, sync_error,
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

    pub async fn list_relays_for_tenant(&self, tenant_id: &str) -> Result<Vec<Relay>> {
        let rows = sqlx::query_as::<_, Relay>(
             "SELECT id, tenant, schema, subdomain, plan, status, sync_error,
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
             "SELECT id, tenant, schema, subdomain, plan, status, sync_error,
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
}
