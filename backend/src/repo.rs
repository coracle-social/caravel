use std::path::Path;
use std::str::FromStr;

use anyhow::Result;
use sqlx::{
    Sqlite, SqlitePool, Transaction,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

use crate::models::{Activity, Plan, Relay, Tenant};

#[derive(Clone)]
pub struct Repo {
    pub pool: SqlitePool,
}

impl Repo {
    pub async fn new() -> Result<Self> {
        let raw_database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| format!("sqlite://{}/data/caravel.db", env!("CARGO_MANIFEST_DIR")));
        let database_url = normalize_sqlite_url(&raw_database_url);

        if let Some(path) = database_url.strip_prefix("sqlite://")
            && !path.is_empty()
            && path != ":memory:"
            && let Some(parent) = Path::new(path).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        let connect_options =
            SqliteConnectOptions::from_str(&database_url)?.create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(connect_options)
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

    pub async fn create_tenant(&self, tenant: &Tenant) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO tenant (pubkey, nwc_url, created_at)
             VALUES (?, ?, ?)",
        )
        .bind(&tenant.pubkey)
        .bind(&tenant.nwc_url)
        .bind(tenant.created_at)
        .execute(&mut *tx)
        .await?;

        Self::insert_activity(&mut tx, "create_tenant", "tenant", &tenant.pubkey).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn update_tenant(&self, tenant: &Tenant) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE tenant SET nwc_url = ? WHERE pubkey = ?")
            .bind(&tenant.nwc_url)
            .bind(&tenant.pubkey)
            .execute(&mut *tx)
            .await?;

        Self::insert_activity(&mut tx, "update_tenant", "tenant", &tenant.pubkey).await?;

        tx.commit().await?;
        Ok(())
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

    pub async fn mark_relay_synced(&self, relay_id: &str) -> Result<()> {
        sqlx::query("UPDATE relay SET synced = 1, status = 'active', sync_error = '' WHERE id = ?")
            .bind(relay_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn max_activity_at(&self) -> Result<i64> {
        let val = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(created_at) FROM activity",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(val.unwrap_or(0))
    }

    pub async fn list_activity(&self, since: &i64) -> Result<Vec<Activity>> {
        let rows = sqlx::query_as::<_, Activity>(
            "SELECT id, tenant, created_at, activity_type, resource_type, resource_id
             FROM activity
             WHERE created_at > ?
             ORDER BY created_at, id",
        )
        .bind(since)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
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
}

fn normalize_sqlite_url(url: &str) -> String {
    let Some(path) = url.strip_prefix("sqlite://") else {
        return url.to_string();
    };

    if path.is_empty() || path == ":memory:" || Path::new(path).is_absolute() {
        return url.to_string();
    }

    format!("sqlite://{}/{}", env!("CARGO_MANIFEST_DIR"), path)
}
