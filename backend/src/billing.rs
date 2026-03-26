use std::collections::HashMap;

use anyhow::Result;
use chrono::{DateTime, Datelike, Duration, Months, TimeZone, Utc};
use tokio::sync::Mutex;

use crate::models::{Activity, Invoice, InvoiceItem, Relay, Tenant};
use crate::repo::Repo;
use crate::robot::Robot;

#[derive(Clone)]
pub struct Billing {
    nwc_url: String,
    repo: Repo,
    robot: Robot,
    last_activity_at: std::sync::Arc<Mutex<i64>>,
}

impl Billing {
    pub fn new(repo: Repo, robot: Robot) -> Self {
        let nwc_url = std::env::var("NWC_URL").unwrap_or_default();
        Self {
            nwc_url,
            repo,
            robot,
            last_activity_at: std::sync::Arc::new(Mutex::new(0)),
        }
    }

    pub async fn start(self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            if let Err(e) = self.tick().await {
                tracing::error!(error = %e, "billing tick failed");
            }
        }
    }

    pub async fn tick(&self) -> Result<()> {
        let mut since_guard = self.last_activity_at.lock().await;
        let since = *since_guard;
        let activity = self.repo.list_activity(&since, None).await?;
        for a in &activity {
            if matches!(
                a.activity_type.as_str(),
                "create_relay" | "update_relay" | "activate_relay"
            ) {
                self.maybe_reset_anchor_for_first_paid_relay(a).await?;
            }
            *since_guard = (*since_guard).max(a.created_at);
        }
        drop(since_guard);

        let tenants = self.repo.list_tenants().await?;
        for tenant in &tenants {
            self.generate_invoice_if_due(tenant).await?;
            self.collect_outstanding(tenant).await?;
        }

        Ok(())
    }

    async fn maybe_reset_anchor_for_first_paid_relay(&self, activity: &Activity) -> Result<()> {
        let relay = match self.repo.get_relay(&activity.resource_id).await? {
            Some(r) => r,
            None => return Ok(()),
        };
        if relay.plan == "free" {
            return Ok(());
        }

        let relays = self.repo.list_relays(Some(&relay.tenant)).await?;
        let paid_active_count = relays
            .into_iter()
            .filter(|r| r.status == "active" && r.plan != "free")
            .count() as i64;

        if paid_active_count == 1 {
            self.repo
                .update_tenant_billing_anchor(&relay.tenant, activity.created_at)
                .await?;
        }

        Ok(())
    }

    async fn generate_invoice_if_due(&self, tenant: &Tenant) -> Result<()> {
        if self
            .repo
            .total_pending_invoices_for_tenant(&tenant.pubkey)
            .await?
            > 0
        {
            return Ok(());
        }

        let relays = self.repo.list_relays(Some(&tenant.pubkey)).await?;
        let active_paid_relays: Vec<Relay> = relays
            .iter()
            .filter(|r| r.status == "active" && r.plan != "free")
            .cloned()
            .collect();
        if active_paid_relays.is_empty() {
            return Ok(());
        }

        let now = Utc::now();
        let anchor = ts_to_dt(tenant.billing_anchor)?;
        let (period_start, period_end) = billing_window(anchor, now);
        if now < period_end {
            return Ok(());
        }

        let usage_events = self
            .repo
            .list_activity(&tenant.billing_anchor, Some(&tenant.pubkey))
            .await?;
        let invoice_id = uuid::Uuid::new_v4().to_string();
        let mut items = Vec::new();

        for relay in active_paid_relays {
            let hours =
                relay_active_hours_in_window(&relay, &usage_events, period_start, period_end);
            if hours <= 0 {
                continue;
            }
            let plan_monthly = self.repo.get_relay_plan_amount_sats(&relay.plan).await?;
            if plan_monthly <= 0 {
                continue;
            }

            let sats = ((plan_monthly as f64 / 30.0 / 24.0) * hours as f64).ceil() as i64;
            if sats <= 0 {
                continue;
            }

            items.push(InvoiceItem {
                id: uuid::Uuid::new_v4().to_string(),
                invoice: invoice_id.clone(),
                relay: relay.id,
                sats,
            });
        }

        let total: i64 = items.iter().map(|i| i.sats).sum();
        if total == 0 {
            return Ok(());
        }

        let bolt11 = match self.make_bolt11(total).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(tenant = %tenant.pubkey, error = %e, "bolt11 generation failed");
                return Ok(());
            }
        };

        let invoice = Invoice {
            id: invoice_id,
            tenant: tenant.pubkey.clone(),
            status: "pending".to_string(),
            created_at: now.timestamp(),
            attempted_at: 0,
            error: String::new(),
            closed_at: 0,
            sent_at: 0,
            paid_at: 0,
            bolt11,
            period_start: period_start.timestamp(),
            period_end: period_end.timestamp(),
        };

        self.repo.create_invoice(&invoice, &items).await?;
        Ok(())
    }

    async fn collect_outstanding(&self, tenant: &Tenant) -> Result<()> {
        let invoices = self.repo.list_invoices(Some(&tenant.pubkey)).await?;
        let now = now_ts();

        for invoice in invoices.into_iter().filter(|i| i.status == "pending") {
            if invoice.attempted_at > 0 && now - invoice.attempted_at < 24 * 3600 {
                continue;
            }

            if self.is_bolt11_paid(&invoice.bolt11).await {
                self.repo.mark_invoice_paid(&invoice.id).await?;
                continue;
            }

            let mut collected = false;
            if !tenant.nwc_url.trim().is_empty()
                && self.pay_invoice_nwc(&tenant.nwc_url, &invoice.bolt11).await
            {
                self.repo.mark_invoice_paid(&invoice.id).await?;
                collected = true;
            }

            if !collected {
                self.repo
                    .mark_invoice_attempted(&invoice.id, Some("autopay failed or unavailable"))
                    .await?;

                if invoice.sent_at == 0 {
                    let amount: i64 = self
                        .repo
                        .get_invoice_items(&invoice.id)
                        .await?
                        .into_iter()
                        .map(|i| i.sats)
                        .sum();
                    let message = format!(
                        "Invoice {} is due. Amount: {} sats\n{}",
                        invoice.id, amount, invoice.bolt11
                    );
                    if self.robot.send_dm(&tenant.pubkey, &message).await.is_ok() {
                        self.repo.mark_invoice_sent(&invoice.id).await?;
                    }
                }
            }

            if now - invoice.created_at >= 7 * 24 * 3600 {
                self.repo.mark_invoice_closed(&invoice.id).await?;
            }
        }

        Ok(())
    }

    async fn make_bolt11(&self, amount_sats: i64) -> Result<String> {
        if self.nwc_url.trim().is_empty() {
            anyhow::bail!("NWC_URL not configured")
        }

        let uri = nostr_sdk::nips::nip47::NostrWalletConnectURI::parse(&self.nwc_url)?;
        let req = nostr_sdk::nips::nip47::Request::make_invoice(
            nostr_sdk::nips::nip47::MakeInvoiceRequest {
                amount: (amount_sats as u64) * 1_000,
                description: Some("Caravel relay invoice".to_string()),
                description_hash: None,
                expiry: None,
            },
        );

        let resp = self.send_nwc_request(&uri, req).await?;
        Ok(resp.to_make_invoice()?.invoice)
    }

    async fn is_bolt11_paid(&self, _bolt11: &str) -> bool {
        false
    }

    async fn pay_invoice_nwc(&self, nwc_url: &str, bolt11: &str) -> bool {
        let uri = match nostr_sdk::nips::nip47::NostrWalletConnectURI::parse(nwc_url) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let req = nostr_sdk::nips::nip47::Request::pay_invoice(
            nostr_sdk::nips::nip47::PayInvoiceRequest::new(bolt11),
        );
        self.send_nwc_request(&uri, req)
            .await
            .and_then(|r| r.to_pay_invoice().map(|_| ()).map_err(anyhow::Error::from))
            .is_ok()
    }

    async fn send_nwc_request(
        &self,
        uri: &nostr_sdk::nips::nip47::NostrWalletConnectURI,
        request: nostr_sdk::nips::nip47::Request,
    ) -> Result<nostr_sdk::nips::nip47::Response> {
        use nostr_sdk::{Client, Filter, Keys, Kind, Timestamp};

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

        let events = client
            .fetch_events(filter, std::time::Duration::from_secs(10))
            .await?;
        let event = events
            .into_iter()
            .max_by_key(|e| e.created_at)
            .ok_or_else(|| anyhow::anyhow!("no NWC response received"))?;

        Ok(nostr_sdk::nips::nip47::Response::from_event(uri, &event)?)
    }
}

fn now_ts() -> i64 {
    Utc::now().timestamp()
}

fn ts_to_dt(ts: i64) -> Result<DateTime<Utc>> {
    Utc.timestamp_opt(ts, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("invalid unix timestamp"))
}

fn billing_window(anchor: DateTime<Utc>, now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let mut start = anchor;
    loop {
        let end = start + Months::new(1);
        if now < end {
            return (start, end);
        }
        start = end;
    }
}

fn relay_active_hours_in_window(
    relay: &Relay,
    events: &[Activity],
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> i64 {
    if relay.plan == "free" {
        return 0;
    }

    let mut marks: HashMap<&str, Vec<&Activity>> = HashMap::new();
    for event in events {
        if event.resource_type == "relay" && event.resource_id == relay.id {
            marks.entry(&relay.id).or_default().push(event);
        }
    }

    let Some(entries) = marks.get(relay.id.as_str()) else {
        if relay.status == "active" {
            return ((end - start).num_seconds() as f64 / 3600.0).ceil() as i64;
        }
        return 0;
    };

    let mut active = relay.status == "active";
    let mut cursor = start;
    let mut secs = 0i64;

    for event in entries.iter().copied() {
        let ts = match Utc.timestamp_opt(event.created_at, 0).single() {
            Some(v) => v,
            None => continue,
        };
        if ts <= start || ts >= end {
            continue;
        }

        match event.activity_type.as_str() {
            "create_relay" | "activate_relay" => {
                if !active {
                    active = true;
                    cursor = ts;
                }
            }
            "deactivate_relay" | "fail_relay_sync" => {
                if active {
                    active = false;
                    secs += (ts - cursor).num_seconds().max(0);
                }
            }
            _ => {}
        }
    }

    if active {
        secs += (end - cursor).num_seconds().max(0);
    }

    let hours = (secs as f64 / 3600.0).ceil() as i64;
    if hours > 0 { hours } else { 0 }
}

#[allow(dead_code)]
fn _same_month(a: DateTime<Utc>, b: DateTime<Utc>) -> bool {
    a.year() == b.year() && a.month() == b.month()
}

#[allow(dead_code)]
fn _days_between(a: i64, b: i64) -> i64 {
    let da = Utc.timestamp_opt(a, 0).single().unwrap_or_else(Utc::now);
    let db = Utc.timestamp_opt(b, 0).single().unwrap_or_else(Utc::now);
    (db - da).num_days()
}

#[allow(dead_code)]
fn _hours_between(a: DateTime<Utc>, b: DateTime<Utc>) -> i64 {
    ((b - a).num_seconds() as f64 / 3600.0).ceil() as i64
}

#[allow(dead_code)]
fn _next_day(dt: DateTime<Utc>) -> DateTime<Utc> {
    dt + Duration::days(1)
}
