use anyhow::Result;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

use crate::models::{
    Invoice, InvoiceItem, NewInvoice, NewInvoiceItem, NewTenant, Relay, Tenant,
};

fn relay_from_row(row: sqlx::sqlite::SqliteRow) -> Relay {
    let config_json: Option<String> = row.get("config");
    let config = config_json.and_then(|s| serde_json::from_str(&s).ok());
    Relay {
        id: row.get("id"),
        tenant: row.get("tenant"),
        name: row.get("name"),
        subdomain: row.get("subdomain"),
        icon: row.get("icon"),
        description: row.get("description"),
        plan: row.get("plan"),
        status: row.get("status"),
        config,
    }
}

#[derive(Clone)]
pub struct Repo {
    pool: SqlitePool,
}

impl Repo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create_tenant(&self, tenant: &NewTenant) -> Result<()> {
        sqlx::query("INSERT INTO tenants (pubkey, status, tenant_nwc_url) VALUES (?, ?, ?)")
            .bind(&tenant.pubkey)
            .bind(&tenant.status)
            .bind(&tenant.tenant_nwc_url)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_tenant_if_missing(&self, tenant: &NewTenant) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO tenants (pubkey, status, tenant_nwc_url) VALUES (?, ?, ?)",
        )
        .bind(&tenant.pubkey)
        .bind(&tenant.status)
        .bind(&tenant.tenant_nwc_url)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_tenant(&self, pubkey: &str) -> Result<Option<Tenant>> {
        let tenant = sqlx::query_as::<_, Tenant>(
            "SELECT pubkey, status, tenant_nwc_url FROM tenants WHERE pubkey = ?",
        )
        .bind(pubkey)
        .fetch_optional(&self.pool)
        .await?;
        Ok(tenant)
    }

    pub async fn list_tenants(&self) -> Result<Vec<Tenant>> {
        let tenants = sqlx::query_as::<_, Tenant>(
            "SELECT pubkey, status, tenant_nwc_url FROM tenants ORDER BY pubkey",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(tenants)
    }

    pub async fn update_tenant_status(&self, pubkey: &str, status: &str) -> Result<()> {
        sqlx::query("UPDATE tenants SET status = ? WHERE pubkey = ?")
            .bind(status)
            .bind(pubkey)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_tenant_nwc_url(&self, pubkey: &str, tenant_nwc_url: &str) -> Result<()> {
        sqlx::query("UPDATE tenants SET tenant_nwc_url = ? WHERE pubkey = ?")
            .bind(tenant_nwc_url)
            .bind(pubkey)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn upsert_relay(&self, relay: &Relay) -> Result<()> {
        let config_json = relay.config.as_ref().map(serde_json::to_string).transpose()?;
        sqlx::query(
            "INSERT INTO relays (id, tenant, name, subdomain, icon, description, plan, status, config)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               subdomain = excluded.subdomain,
               icon = excluded.icon,
               description = excluded.description,
               plan = excluded.plan,
               status = excluded.status,
               config = excluded.config",
        )
        .bind(&relay.id)
        .bind(&relay.tenant)
        .bind(&relay.name)
        .bind(&relay.subdomain)
        .bind(&relay.icon)
        .bind(&relay.description)
        .bind(&relay.plan)
        .bind(&relay.status)
        .bind(config_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_relay_status(&self, id: &str, status: &str) -> Result<()> {
        sqlx::query("UPDATE relays SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn suspend_relays_for_tenant(&self, tenant: &str) -> Result<()> {
        sqlx::query(
            "UPDATE relays SET status = 'suspended' WHERE tenant = ? AND status = 'active'",
        )
        .bind(tenant)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_relay(&self, id: &str) -> Result<Option<Relay>> {
        let row = sqlx::query(
            "SELECT id, tenant, name, subdomain, icon, description, plan, status, config FROM relays WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(relay_from_row))
    }

    pub async fn list_relays_by_tenant(&self, tenant: &str) -> Result<Vec<Relay>> {
        let rows = sqlx::query(
            "SELECT id, tenant, name, subdomain, icon, description, plan, status, config FROM relays WHERE tenant = ? ORDER BY name",
        )
        .bind(tenant)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(relay_from_row).collect())
    }

    pub async fn list_relays(&self) -> Result<Vec<Relay>> {
        let rows = sqlx::query(
            "SELECT id, tenant, name, subdomain, icon, description, plan, status, config FROM relays ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(relay_from_row).collect())
    }

    pub async fn create_invoice_with_items(
        &self,
        invoice: &NewInvoice,
        items: &[NewInvoiceItem],
    ) -> Result<()> {
        let mut tx: Transaction<'_, Sqlite> = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO invoices (id, tenant, amount, status, created_at, invoice)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&invoice.id)
        .bind(&invoice.tenant)
        .bind(invoice.amount)
        .bind(&invoice.status)
        .bind(&invoice.created_at)
        .bind(&invoice.invoice)
        .execute(&mut *tx)
        .await?;

        for item in items {
            sqlx::query(
                "INSERT INTO invoice_items (id, invoice, relay, amount, period_start, period_end)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&item.id)
            .bind(&item.invoice)
            .bind(&item.relay)
            .bind(item.amount)
            .bind(&item.period_start)
            .bind(&item.period_end)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn list_invoices_by_tenant(&self, tenant: &str) -> Result<Vec<Invoice>> {
        let invoices = sqlx::query_as::<_, Invoice>(
            "SELECT id, tenant, amount, status, created_at, invoice FROM invoices WHERE tenant = ? ORDER BY created_at DESC",
        )
        .bind(tenant)
        .fetch_all(&self.pool)
        .await?;
        Ok(invoices)
    }

    pub async fn list_invoice_items(&self, invoice_id: &str) -> Result<Vec<InvoiceItem>> {
        let items = sqlx::query_as::<_, InvoiceItem>(
            "SELECT id, invoice, relay, amount, period_start, period_end FROM invoice_items WHERE invoice = ?",
        )
        .bind(invoice_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(items)
    }

    pub async fn update_invoice_status(&self, id: &str, status: &str) -> Result<()> {
        sqlx::query("UPDATE invoices SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
