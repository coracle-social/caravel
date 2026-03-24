use anyhow::Result;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

use crate::models::{
    Invoice, InvoiceAttempt, InvoiceItem, Plan, Relay, RelayLifecycleEvent, Tenant,
};

// ── helpers ──────────────────────────────────────────────────────────────────

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

const TENANT_COLS: &str =
    "pubkey, status, nwc_url, created_at, billing_anchor_at, stripe_customer_id, stripe_subscription_id";

const RELAY_COLS: &str = "id, tenant, name, subdomain, icon, description, plan, status, config";

const INVOICE_COLS: &str =
    "id, tenant, amount, status, created_at, bolt11, period_start, period_end";

const LIFECYCLE_COLS: &str = "id, relay, tenant, event_type, plan, created_at";

const ATTEMPT_COLS: &str = "id, invoice, run_id, method, outcome, error, created_at";

// ── Repo ─────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Repo {
    pool: SqlitePool,
}

impl Repo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // ── tenants ──────────────────────────────────────────────────────────────

    pub async fn create_tenant(&self, tenant: &Tenant) -> Result<()> {
        sqlx::query(
            "INSERT INTO tenants (pubkey, status, nwc_url, created_at, billing_anchor_at,
                                  stripe_customer_id, stripe_subscription_id)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&tenant.pubkey)
        .bind(&tenant.status)
        .bind(&tenant.nwc_url)
        .bind(tenant.created_at)
        .bind(tenant.billing_anchor_at)
        .bind(&tenant.stripe_customer_id)
        .bind(&tenant.stripe_subscription_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_tenant_if_missing(&self, tenant: &Tenant) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO tenants
               (pubkey, status, nwc_url, created_at, billing_anchor_at,
                stripe_customer_id, stripe_subscription_id)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&tenant.pubkey)
        .bind(&tenant.status)
        .bind(&tenant.nwc_url)
        .bind(tenant.created_at)
        .bind(tenant.billing_anchor_at)
        .bind(&tenant.stripe_customer_id)
        .bind(&tenant.stripe_subscription_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_tenant(&self, pubkey: &str) -> Result<Option<Tenant>> {
        let tenant = sqlx::query_as::<_, Tenant>(&format!(
            "SELECT {TENANT_COLS} FROM tenants WHERE pubkey = ?"
        ))
        .bind(pubkey)
        .fetch_optional(&self.pool)
        .await?;
        Ok(tenant)
    }

    pub async fn list_tenants(&self) -> Result<Vec<Tenant>> {
        let tenants = sqlx::query_as::<_, Tenant>(&format!(
            "SELECT {TENANT_COLS} FROM tenants ORDER BY pubkey"
        ))
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

    pub async fn update_tenant_nwc_url(&self, pubkey: &str, nwc_url: &str) -> Result<()> {
        sqlx::query("UPDATE tenants SET nwc_url = ? WHERE pubkey = ?")
            .bind(nwc_url)
            .bind(pubkey)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn update_tenant_billing_integrations(
        &self,
        pubkey: &str,
        nwc_url: &str,
        stripe_customer_id: &str,
        stripe_subscription_id: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE tenants
             SET nwc_url = ?, stripe_customer_id = ?, stripe_subscription_id = ?
             WHERE pubkey = ?",
        )
        .bind(nwc_url)
        .bind(stripe_customer_id)
        .bind(stripe_subscription_id)
        .bind(pubkey)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Reset the billing cycle anchor. Called when a tenant adds their first
    /// paid relay after having none.
    pub async fn reset_billing_anchor(&self, pubkey: &str, anchor_at: i64) -> Result<()> {
        sqlx::query("UPDATE tenants SET billing_anchor_at = ? WHERE pubkey = ?")
            .bind(anchor_at)
            .bind(pubkey)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Returns the count of non-free, non-deactivated relays for a tenant.
    pub async fn count_billable_relays(&self, tenant: &str) -> Result<i64> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM relays
             WHERE tenant = ? AND plan != 'free' AND status != 'deactivated'",
        )
        .bind(tenant)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    // ── relays ────────────────────────────────────────────────────────────────

    pub async fn upsert_relay(&self, relay: &Relay) -> Result<()> {
        let config_json = relay
            .config
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        sqlx::query(&format!(
            "INSERT INTO relays ({RELAY_COLS})
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               name        = excluded.name,
               subdomain   = excluded.subdomain,
               icon        = excluded.icon,
               description = excluded.description,
               plan        = excluded.plan,
               status      = excluded.status,
               config      = excluded.config"
        ))
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

    pub async fn get_relay(&self, id: &str) -> Result<Option<Relay>> {
        let row = sqlx::query(&format!("SELECT {RELAY_COLS} FROM relays WHERE id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(relay_from_row))
    }

    pub async fn list_relays_by_tenant(&self, tenant: &str) -> Result<Vec<Relay>> {
        let rows = sqlx::query(&format!(
            "SELECT {RELAY_COLS} FROM relays WHERE tenant = ? ORDER BY name"
        ))
        .bind(tenant)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(relay_from_row).collect())
    }

    pub async fn list_relays(&self) -> Result<Vec<Relay>> {
        let rows = sqlx::query(&format!("SELECT {RELAY_COLS} FROM relays ORDER BY name"))
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(relay_from_row).collect())
    }

    // ── lifecycle events ──────────────────────────────────────────────────────

    /// All lifecycle events for a tenant up to (exclusive) a given timestamp,
    /// ordered by relay then time — used by the invoice generation worker.
    pub async fn list_lifecycle_events_for_tenant(
        &self,
        tenant: &str,
        before: i64,
    ) -> Result<Vec<RelayLifecycleEvent>> {
        let rows = sqlx::query_as::<_, RelayLifecycleEvent>(&format!(
            "SELECT {LIFECYCLE_COLS} FROM relay_lifecycle_events
             WHERE tenant = ? AND created_at < ?
             ORDER BY relay, created_at, id"
        ))
        .bind(tenant)
        .bind(before)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // ── plans ─────────────────────────────────────────────────────────────────

    pub async fn list_plans(&self) -> Result<Vec<Plan>> {
        let plans =
            sqlx::query_as::<_, Plan>("SELECT id, sats_per_month FROM plans ORDER BY sats_per_month")
                .fetch_all(&self.pool)
                .await?;
        Ok(plans)
    }

    #[allow(dead_code)]
    pub async fn get_plan(&self, id: &str) -> Result<Option<Plan>> {
        let plan = sqlx::query_as::<_, Plan>("SELECT id, sats_per_month FROM plans WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(plan)
    }

    // ── invoices ──────────────────────────────────────────────────────────────

    /// Insert an invoice and its line items atomically.
    /// Returns `false` (no-op) if an invoice for this tenant+period already exists.
    pub async fn create_invoice_with_items(
        &self,
        invoice: &Invoice,
        items: &[InvoiceItem],
    ) -> Result<bool> {
        let mut tx: Transaction<'_, Sqlite> = self.pool.begin().await?;

        let result = sqlx::query(
            "INSERT INTO invoices (id, tenant, amount, status, created_at, bolt11, period_start, period_end)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(tenant, period_start, period_end) DO NOTHING",
        )
        .bind(&invoice.id)
        .bind(&invoice.tenant)
        .bind(invoice.amount)
        .bind(&invoice.status)
        .bind(invoice.created_at)
        .bind(&invoice.bolt11)
        .bind(invoice.period_start)
        .bind(invoice.period_end)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }

        for item in items {
            sqlx::query(
                "INSERT INTO invoice_items (id, invoice, relay, amount, period_start, period_end)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&item.id)
            .bind(&item.invoice)
            .bind(&item.relay)
            .bind(item.amount)
            .bind(item.period_start)
            .bind(item.period_end)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(true)
    }

    #[allow(dead_code)]
    pub async fn get_invoice(&self, id: &str) -> Result<Option<Invoice>> {
        let invoice = sqlx::query_as::<_, Invoice>(&format!(
            "SELECT {INVOICE_COLS} FROM invoices WHERE id = ?"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(invoice)
    }

    pub async fn list_invoices_by_tenant(&self, tenant: &str) -> Result<Vec<Invoice>> {
        let invoices = sqlx::query_as::<_, Invoice>(&format!(
            "SELECT {INVOICE_COLS} FROM invoices WHERE tenant = ? ORDER BY created_at DESC"
        ))
        .bind(tenant)
        .fetch_all(&self.pool)
        .await?;
        Ok(invoices)
    }

    #[allow(dead_code)]
    pub async fn list_invoice_items(&self, invoice_id: &str) -> Result<Vec<InvoiceItem>> {
        let items = sqlx::query_as::<_, InvoiceItem>(
            "SELECT id, invoice, relay, amount, period_start, period_end
             FROM invoice_items WHERE invoice = ?",
        )
        .bind(invoice_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(items)
    }

    // ── invoice attempts ──────────────────────────────────────────────────────

    /// Record a payment attempt and synchronously project the new invoice status.
    pub async fn record_attempt(
        &self,
        attempt: &InvoiceAttempt,
        projected_invoice_status: &str,
    ) -> Result<()> {
        let mut tx: Transaction<'_, Sqlite> = self.pool.begin().await?;

        sqlx::query(&format!(
            "INSERT INTO invoice_attempts ({ATTEMPT_COLS}) VALUES (?, ?, ?, ?, ?, ?, ?)"
        ))
        .bind(&attempt.id)
        .bind(&attempt.invoice)
        .bind(&attempt.run_id)
        .bind(&attempt.method)
        .bind(&attempt.outcome)
        .bind(&attempt.error)
        .bind(attempt.created_at)
        .execute(&mut *tx)
        .await?;

        sqlx::query("UPDATE invoices SET status = ? WHERE id = ?")
            .bind(projected_invoice_status)
            .bind(&attempt.invoice)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn list_attempts_for_invoice(&self, invoice_id: &str) -> Result<Vec<InvoiceAttempt>> {
        let attempts = sqlx::query_as::<_, InvoiceAttempt>(&format!(
            "SELECT {ATTEMPT_COLS} FROM invoice_attempts
             WHERE invoice = ?
             ORDER BY created_at ASC, id ASC"
        ))
        .bind(invoice_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(attempts)
    }

    /// Returns true if a DM has already been sent for this invoice (to enforce
    /// the one-DM-per-invoice rule).
    pub async fn invoice_dm_sent(&self, invoice_id: &str) -> Result<bool> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM invoice_attempts
             WHERE invoice = ? AND method = 'nip17_dm' AND outcome = 'sent'",
        )
        .bind(invoice_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    // ── relay lifecycle + status: transactional helpers ───────────────────────

    /// Transition a relay's status and write the corresponding lifecycle event
    /// atomically. No-ops if the relay is already in `new_status`.
    pub async fn transition_relay(
        &self,
        relay_id: &str,
        tenant_pubkey: &str,
        plan: &str,
        new_status: &str,
        event_type: &str,
        now: i64,
    ) -> Result<bool> {
        let mut tx: Transaction<'_, Sqlite> = self.pool.begin().await?;

        let current: Option<String> = sqlx::query_scalar("SELECT status FROM relays WHERE id = ?")
            .bind(relay_id)
            .fetch_optional(&mut *tx)
            .await?;

        let current_status = match current {
            Some(s) => s,
            None => return Ok(false),
        };

        if current_status == new_status {
            return Ok(true); // idempotent no-op
        }

        sqlx::query("UPDATE relays SET status = ? WHERE id = ?")
            .bind(new_status)
            .bind(relay_id)
            .execute(&mut *tx)
            .await?;

        let event = RelayLifecycleEvent {
            id: uuid::Uuid::new_v4().to_string(),
            relay: relay_id.to_string(),
            tenant: tenant_pubkey.to_string(),
            event_type: event_type.to_string(),
            plan: plan.to_string(),
            created_at: now,
        };

        sqlx::query(&format!(
            "INSERT INTO relay_lifecycle_events ({LIFECYCLE_COLS}) VALUES (?, ?, ?, ?, ?, ?)"
        ))
        .bind(&event.id)
        .bind(&event.relay)
        .bind(&event.tenant)
        .bind(&event.event_type)
        .bind(&event.plan)
        .bind(event.created_at)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(true)
    }

    /// Suspend all active relays for a tenant, writing lifecycle events for each.
    pub async fn suspend_relays_for_tenant(&self, tenant: &str, now: i64) -> Result<()> {
        let mut tx: Transaction<'_, Sqlite> = self.pool.begin().await?;

        let rows =
            sqlx::query("SELECT id, plan FROM relays WHERE tenant = ? AND status = 'active'")
                .bind(tenant)
                .fetch_all(&mut *tx)
                .await?;

        for row in rows {
            let relay_id: String = row.get("id");
            let plan: String = row.get("plan");

            sqlx::query("UPDATE relays SET status = 'suspended' WHERE id = ?")
                .bind(&relay_id)
                .execute(&mut *tx)
                .await?;

            sqlx::query(&format!(
                "INSERT INTO relay_lifecycle_events ({LIFECYCLE_COLS}) VALUES (?, ?, ?, ?, ?, ?)"
            ))
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&relay_id)
            .bind(tenant)
            .bind("suspended")
            .bind(plan)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Reactivate all billing-suspended relays for a tenant (used after full
    /// balance payment).
    pub async fn reactivate_relays_for_tenant(&self, tenant: &str, now: i64) -> Result<()> {
        let mut tx: Transaction<'_, Sqlite> = self.pool.begin().await?;

        let rows =
            sqlx::query("SELECT id, plan FROM relays WHERE tenant = ? AND status = 'suspended'")
                .bind(tenant)
                .fetch_all(&mut *tx)
                .await?;

        for row in rows {
            let relay_id: String = row.get("id");
            let plan: String = row.get("plan");

            sqlx::query("UPDATE relays SET status = 'active' WHERE id = ?")
                .bind(&relay_id)
                .execute(&mut *tx)
                .await?;

            sqlx::query(&format!(
                "INSERT INTO relay_lifecycle_events ({LIFECYCLE_COLS}) VALUES (?, ?, ?, ?, ?, ?)"
            ))
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&relay_id)
            .bind(tenant)
            .bind("unsuspended")
            .bind(plan)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}
