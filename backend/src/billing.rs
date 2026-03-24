use anyhow::{Result, anyhow};
use chrono::{DateTime, Months, TimeZone, Utc};
use std::collections::HashMap;
use tokio::time::{Duration, sleep};
use uuid::Uuid;

use crate::models::{Invoice, InvoiceAttempt, InvoiceItem, RelayLifecycleEvent, Tenant};
use crate::notifications::Nip17Notifier;
use crate::repo::Repo;

use nostr_sdk::nips::nip47::{self, MakeInvoiceRequest, NostrWalletConnectURI, PayInvoiceRequest};
use nostr_sdk::{Client, Filter, Keys, Kind, Timestamp};

const GRACE_DAYS: i64 = 7;
const DUE_DAYS: i64 = 7;
const WORKER_INTERVAL_SECS: u64 = 300;

// ── service ───────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct BillingService {
    repo: Repo,
    notifier: Nip17Notifier,
    platform_nwc_url: String,
}

impl BillingService {
    pub fn new(repo: Repo, notifier: Nip17Notifier, platform_nwc_url: String) -> Self {
        Self {
            repo,
            notifier,
            platform_nwc_url,
        }
    }

    pub async fn run(self) {
        loop {
            if let Err(err) = self.process_once().await {
                tracing::error!(error = %err, "billing run failed");
            }
            sleep(Duration::from_secs(WORKER_INTERVAL_SECS)).await;
        }
    }

    async fn process_once(&self) -> Result<()> {
        let tenants = self.repo.list_tenants().await?;
        for tenant in &tenants {
            if let Err(err) = self.generate_invoice_if_due(tenant).await {
                tracing::error!(tenant = %tenant.pubkey, error = %err, "invoice generation failed");
            }
        }
        for tenant in &tenants {
            if let Err(err) = self.collect_outstanding(tenant).await {
                tracing::error!(tenant = %tenant.pubkey, error = %err, "collection failed");
            }
        }
        Ok(())
    }

    // ── invoice generation ────────────────────────────────────────────────────

    async fn generate_invoice_if_due(&self, tenant: &Tenant) -> Result<()> {
        if tenant.status != "active" {
            return Ok(());
        }

        let anchor = ts_to_dt(tenant.billing_anchor_at)?;
        let now = Utc::now();
        let (period_start, period_end) = current_billing_period(anchor, now);

        // Only generate once the period has closed
        if now < period_end {
            return Ok(());
        }

        let plans = self.repo.list_plans().await?;
        let plan_amount_map: HashMap<String, i64> =
            plans.into_iter().map(|p| (p.id, p.sats_per_month)).collect();

        let events = self
            .repo
            .list_lifecycle_events_for_tenant(&tenant.pubkey, dt_to_ts(period_end))
            .await?;

        let invoice_id = Uuid::new_v4().to_string();
        let items = compute_invoice_items(
            &invoice_id,
            &events,
            &plan_amount_map,
            period_start,
            period_end,
        );

        let total: i64 = items.iter().map(|i| i.amount).sum();
        if total == 0 {
            return Ok(());
        }

        let bolt11 = self.make_bolt11(total).await.unwrap_or_default();
        let invoice = Invoice {
            id: invoice_id.clone(),
            tenant: tenant.pubkey.clone(),
            amount: total,
            status: "pending".to_string(),
            created_at: dt_to_ts(now),
            bolt11,
            period_start: dt_to_ts(period_start),
            period_end: dt_to_ts(period_end),
        };

        let created = self
            .repo
            .create_invoice_with_items(&invoice, &items)
            .await?;

        if created {
            tracing::info!(tenant = %tenant.pubkey, invoice = %invoice_id, amount = total, "invoice generated");
        }

        Ok(())
    }

    // ── collection ────────────────────────────────────────────────────────────

    async fn collect_outstanding(&self, tenant: &Tenant) -> Result<()> {
        let invoices = self.repo.list_invoices_by_tenant(&tenant.pubkey).await?;
        let unpaid: Vec<&Invoice> = invoices
            .iter()
            .filter(|inv| matches!(inv.status.as_str(), "pending" | "past_due"))
            .collect();

        if unpaid.is_empty() {
            return Ok(());
        }

        for invoice in &unpaid {
            self.attempt_collection(tenant, invoice).await?;
        }

        // Re-fetch to check if all are now paid; auto-reactivate if so
        let invoices_after = self.repo.list_invoices_by_tenant(&tenant.pubkey).await?;
        let still_unpaid = invoices_after
            .iter()
            .any(|inv| matches!(inv.status.as_str(), "pending" | "past_due"));

        if !still_unpaid && tenant.status == "suspended" {
            let now = now_ts();
            self.repo
                .update_tenant_status(&tenant.pubkey, "active")
                .await?;
            self.repo
                .reactivate_relays_for_tenant(&tenant.pubkey, now)
                .await?;
            tracing::info!(tenant = %tenant.pubkey, "tenant reactivated after full balance payment");
        }

        Ok(())
    }

    async fn attempt_collection(&self, tenant: &Tenant, invoice: &Invoice) -> Result<()> {
        let now = Utc::now();
        let created_at = ts_to_dt(invoice.created_at)?;
        let due_at = created_at + chrono::Duration::days(DUE_DAYS);
        let grace_ends_at = due_at + chrono::Duration::days(GRACE_DAYS);

        // Deactivate after grace period expires
        if now > grace_ends_at && invoice.status != "past_due" {
            let ts = now_ts();
            self.repo
                .update_tenant_status(&tenant.pubkey, "suspended")
                .await?;
            self.repo
                .suspend_relays_for_tenant(&tenant.pubkey, ts)
                .await?;
            self.repo
                .record_attempt(
                    &InvoiceAttempt {
                        id: Uuid::new_v4().to_string(),
                        invoice: invoice.id.clone(),
                        run_id: Uuid::new_v4().to_string(),
                        method: "system".to_string(),
                        outcome: "failed".to_string(),
                        error: "grace period expired".to_string(),
                        created_at: ts,
                    },
                    "past_due",
                )
                .await?;
            return Ok(());
        }

        // Only retry once per 24h
        let attempts = self.repo.list_attempts_for_invoice(&invoice.id).await?;
        if let Some(last) = attempts.last()
            && last.method != "nip17_dm"
        {
            let last_at = ts_to_dt(last.created_at)?;
            if now - last_at < chrono::Duration::hours(24) {
                return Ok(());
            }
        }

        let run_id = Uuid::new_v4().to_string();

        // 1. Try NWC
        if !tenant.nwc_url.trim().is_empty() {
            match self
                .pay_via_nwc(&tenant.nwc_url, &invoice.bolt11)
                .await
            {
                Ok(()) => {
                    self.repo
                        .record_attempt(
                            &attempt(&invoice.id, &run_id, "nwc", "success", ""),
                            "paid",
                        )
                        .await?;
                    return Ok(());
                }
                Err(err) => {
                    tracing::warn!(tenant = %tenant.pubkey, error = %err, "NWC payment failed");
                    self.repo
                        .record_attempt(
                            &attempt(&invoice.id, &run_id, "nwc", "failed", &err.to_string()),
                            &invoice.status,
                        )
                        .await?;
                }
            }
        }

        // 2. Try Stripe
        if !tenant.stripe_subscription_id.trim().is_empty() {
            match self.pay_via_stripe(tenant, invoice).await {
                Ok(()) => {
                    self.repo
                        .record_attempt(
                            &attempt(&invoice.id, &run_id, "stripe", "success", ""),
                            "paid",
                        )
                        .await?;
                    return Ok(());
                }
                Err(err) => {
                    tracing::warn!(tenant = %tenant.pubkey, error = %err, "Stripe payment failed");
                    self.repo
                        .record_attempt(
                            &attempt(&invoice.id, &run_id, "stripe", "failed", &err.to_string()),
                            &invoice.status,
                        )
                        .await?;
                }
            }
        }

        // 3. Fallback: Lightning invoice shown in-app; send one DM if no auto-pay configured
        let dm_sent = self.repo.invoice_dm_sent(&invoice.id).await?;
        if !dm_sent {
            match self
                .send_invoice_dm(tenant, invoice, invoice.bolt11.as_str())
                .await
            {
                Ok(()) => {
                    self.repo
                        .record_attempt(
                            &attempt(&invoice.id, &run_id, "nip17_dm", "sent", ""),
                            &invoice.status,
                        )
                        .await?;
                }
                Err(err) => {
                    tracing::warn!(tenant = %tenant.pubkey, error = %err, "NIP-17 DM failed");
                }
            }
        }

        Ok(())
    }

    // ── payment providers ─────────────────────────────────────────────────────

    async fn make_bolt11(&self, amount_sats: i64) -> Result<String> {
        if self.platform_nwc_url.trim().is_empty() {
            return Err(anyhow!("NWC_URL is required to generate invoices"));
        }
        let uri = NostrWalletConnectURI::parse(&self.platform_nwc_url)?;
        let req = nip47::Request::make_invoice(MakeInvoiceRequest {
            amount: (amount_sats as u64) * 1_000,
            description: Some("Relay hosting".to_string()),
            description_hash: None,
            expiry: None,
        });
        let resp = self.send_nwc_request(&uri, req).await?;
        Ok(resp.to_make_invoice()?.invoice)
    }

    async fn pay_via_nwc(&self, nwc_url: &str, bolt11: &str) -> Result<()> {
        let uri = NostrWalletConnectURI::parse(nwc_url)?;
        let req = nip47::Request::pay_invoice(PayInvoiceRequest::new(bolt11));
        self.send_nwc_request(&uri, req).await?.to_pay_invoice()?;
        Ok(())
    }

    async fn pay_via_stripe(&self, _tenant: &Tenant, _invoice: &Invoice) -> Result<()> {
        // TODO: implement Stripe off-session charge using tenant.stripe_subscription_id
        Err(anyhow!("Stripe not yet implemented"))
    }

    async fn send_nwc_request(
        &self,
        uri: &NostrWalletConnectURI,
        request: nip47::Request,
    ) -> Result<nip47::Response> {
        let app_keys = Keys::new(uri.secret.clone());
        let app_pubkey = app_keys.public_key();
        let client = Client::new(app_keys);
        client.add_relay(uri.relay_url.clone()).await?;
        client.connect().await;

        let started_at = Timestamp::now();
        let event = request.to_event(uri)?;
        client.send_event(event).await?;

        let filter = Filter::new()
            .kind(Kind::WalletConnectResponse)
            .author(uri.public_key)
            .pubkey(app_pubkey)
            .since(started_at);

        let events = client.fetch_events(filter, Duration::from_secs(10)).await?;
        let event = events
            .into_iter()
            .max_by_key(|e| e.created_at)
            .ok_or_else(|| anyhow!("no NWC response received"))?;

        Ok(nip47::Response::from_event(uri, &event)?)
    }

    async fn send_invoice_dm(
        &self,
        tenant: &Tenant,
        invoice: &Invoice,
        bolt11: &str,
    ) -> Result<()> {
        let due_date = ts_to_dt(invoice.created_at + DUE_DAYS * 86400)?;
        let period_start = ts_to_dt(invoice.period_start)?;
        let period_end = ts_to_dt(invoice.period_end)?;
        let message = format!(
            "You have an outstanding invoice of {} sats due by {}.\n\
             Period: {} → {}\n\
             Pay with Lightning:\n{}",
            invoice.amount,
            due_date.format("%Y-%m-%d"),
            period_start.format("%Y-%m-%d"),
            period_end.format("%Y-%m-%d"),
            bolt11,
        );
        self.notifier.send(&tenant.pubkey, &message).await
    }
}

// ── billing math ──────────────────────────────────────────────────────────────

/// Given a billing anchor and the current time, return the current billing
/// period [start, end) based on rolling monthly windows from the anchor.
fn current_billing_period(
    anchor: DateTime<Utc>,
    now: DateTime<Utc>,
) -> (DateTime<Utc>, DateTime<Utc>) {
    let mut period_start = anchor;
    loop {
        let period_end = period_start + Months::new(1);
        if now < period_end {
            return (period_start, period_end);
        }
        period_start = period_end;
    }
}

/// Compute per-relay billable sats for a billing period from the lifecycle
/// event log. Rules:
///   - Billing starts at `provisioned`, pauses at `suspended`, resumes at
///     `unsuspended`, stops at `deactivated`.
///   - Only time within [period_start, period_end) counts.
///   - Round each relay's total billable seconds up to the next full hour.
///   - Minimum 1 billable hour per relay per period.
///   - Amount is based on the relay's current plan amount (retroactive within period).
fn compute_invoice_items(
    invoice_id: &str,
    events: &[RelayLifecycleEvent],
    plan_amount_map: &HashMap<String, i64>,
    period_start: DateTime<Utc>,
    period_end: DateTime<Utc>,
) -> Vec<InvoiceItem> {
    // Group events by relay, preserving sort order from the DB (relay, created_at, id)
    let mut by_relay: HashMap<&str, Vec<&RelayLifecycleEvent>> = HashMap::new();
    for event in events {
        by_relay.entry(&event.relay).or_default().push(event);
    }

    let mut items = Vec::new();

    for (relay_id, relay_events) in &by_relay {
        // Use the latest plan for this relay (retroactive rate within period)
        let Some(latest_event) = relay_events.last() else {
            continue;
        };
        let plan_amount = *plan_amount_map.get(latest_event.plan.as_str()).unwrap_or(&0);
        if plan_amount == 0 {
            continue;
        }

        let billable_secs = billable_seconds_in_period(relay_events, period_start, period_end);
        if billable_secs == 0 {
            continue;
        }

        // Round up to next full hour, minimum 1 hour
        let hours = ((billable_secs as f64) / 3600.0).ceil().max(1.0) as i64;

        items.push(InvoiceItem {
            id: Uuid::new_v4().to_string(),
            invoice: invoice_id.to_string(),
            relay: relay_id.to_string(),
            amount: hours * plan_amount,
            period_start: dt_to_ts(period_start),
            period_end: dt_to_ts(period_end),
        });
    }

    items
}

/// Compute total billable seconds for one relay within [period_start, period_end).
/// Replays the full event history to correctly handle events that precede the period.
fn billable_seconds_in_period(
    events: &[&RelayLifecycleEvent],
    period_start: DateTime<Utc>,
    period_end: DateTime<Utc>,
) -> i64 {
    let mut total_secs: i64 = 0;
    let mut billing_start: Option<DateTime<Utc>> = None;

    for event in events {
        let Ok(ts) = ts_to_dt(event.created_at) else {
            continue;
        };

        match event.event_type.as_str() {
            "provisioned" | "unsuspended" => {
                if billing_start.is_none() {
                    billing_start = Some(ts.max(period_start));
                }
            }
            "suspended" | "deactivated" => {
                if let Some(start) = billing_start.take() {
                    let end = ts.min(period_end);
                    if end > start {
                        total_secs += (end - start).num_seconds();
                    }
                }
            }
            _ => {}
        }
    }

    // Still billing at period end
    if let Some(start) = billing_start
        && period_end > start
    {
        total_secs += (period_end - start).num_seconds();
    }

    total_secs
}

// ── helpers ───────────────────────────────────────────────────────────────────

pub fn now_ts() -> i64 {
    Utc::now().timestamp()
}

fn dt_to_ts(dt: DateTime<Utc>) -> i64 {
    dt.timestamp()
}

fn ts_to_dt(ts: i64) -> Result<DateTime<Utc>> {
    Utc.timestamp_opt(ts, 0)
        .single()
        .ok_or_else(|| anyhow!("invalid unix timestamp: {ts}"))
}

fn attempt(
    invoice_id: &str,
    run_id: &str,
    method: &str,
    outcome: &str,
    error: &str,
) -> InvoiceAttempt {
    InvoiceAttempt {
        id: Uuid::new_v4().to_string(),
        invoice: invoice_id.to_string(),
        run_id: run_id.to_string(),
        method: method.to_string(),
        outcome: outcome.to_string(),
        error: error.to_string(),
        created_at: now_ts(),
    }
}
