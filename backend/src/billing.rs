use anyhow::{anyhow, Result};
use chrono::{DateTime, Months, Utc};
use tokio::time::{sleep, Duration};
use uuid::Uuid;

use crate::models::{Invoice, NewInvoice, NewInvoiceItem, Relay, Tenant};
use crate::notifications::Nip17Notifier;
use crate::repo::Repo;

use nostr_sdk::nips::nip47::{self, MakeInvoiceRequest, NostrWalletConnectURI, PayInvoiceRequest};
use nostr_sdk::{Client, Filter, Kind, Keys, Timestamp};

#[derive(Clone)]
pub struct BillingService {
    repo: Repo,
    notifier: Nip17Notifier,
    platform_nwc_url: String,
}

impl BillingService {
    pub fn new(
        repo: Repo,
        notifier: Nip17Notifier,
        platform_nwc_url: String,
    ) -> Self {
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
            sleep(Duration::from_secs(300)).await;
        }
    }

    async fn process_once(&self) -> Result<()> {
        let tenants = self.repo.list_tenants().await?;
        for tenant in tenants {
            if let Err(err) = self.bill_tenant(&tenant).await {
                tracing::error!(tenant = tenant.pubkey, error = %err, "billing failed");
            }
            if let Err(err) = self.suspend_if_delinquent(&tenant).await {
                tracing::error!(tenant = tenant.pubkey, error = %err, "grace period enforcement failed");
            }
        }
        Ok(())
    }

    async fn bill_tenant(&self, tenant: &Tenant) -> Result<()> {
        if tenant.status != "active" {
            return Ok(());
        }

        let relays = self.repo.list_relays_by_tenant(&tenant.pubkey).await?;
        let active_relays = relays
            .into_iter()
            .filter(|relay| relay.status == "active")
            .collect::<Vec<_>>();

        let invoices = self.repo.list_invoices_by_tenant(&tenant.pubkey).await?;

        let (period_start, period_end, should_bill) = next_billing_window(&invoices)?;
        if !should_bill {
            return Ok(());
        }

        let invoice_id = Uuid::new_v4().to_string();
        let items = build_invoice_items(&invoice_id, &active_relays, period_start, period_end);
        let total_amount: i64 = items.iter().map(|item| item.amount).sum();

        if total_amount == 0 {
            return Ok(());
        }

        let invoice_str = self.make_invoice(total_amount).await?;
        let invoice = NewInvoice {
            id: invoice_id.clone(),
            tenant: tenant.pubkey.clone(),
            amount: total_amount,
            status: "pending".to_string(),
            created_at: Utc::now().to_rfc3339(),
            invoice: invoice_str.clone(),
        };

        self.repo.create_invoice_with_items(&invoice, &items).await?;

        if tenant.tenant_nwc_url.trim().is_empty() {
            self.send_invoice_dm(tenant, &invoice, period_start, period_end)
                .await?;
            return Ok(());
        }

        match self.pay_invoice(&tenant.tenant_nwc_url, &invoice_str).await {
            Ok(()) => {
                self.repo.update_invoice_status(&invoice_id, "paid").await?;
                self.send_payment_dm(tenant, &invoice).await?;
            }
            Err(err) => {
                tracing::error!(tenant = tenant.pubkey, error = %err, "recurring payment failed");
            }
        }

        Ok(())
    }

    async fn suspend_if_delinquent(&self, tenant: &Tenant) -> Result<()> {
        if tenant.status != "active" {
            return Ok(());
        }

        let invoices = self.repo.list_invoices_by_tenant(&tenant.pubkey).await?;
        let latest = match invoices.first() {
            Some(invoice) => invoice,
            None => return Ok(()),
        };

        if latest.status != "pending" {
            return Ok(());
        }

        let created_at = parse_timestamp(&latest.created_at)?;
        let deadline = created_at + chrono::Duration::days(7);
        if Utc::now() < deadline {
            return Ok(());
        }

        self.repo.update_tenant_status(&tenant.pubkey, "suspended").await?;
        self.repo.suspend_relays_for_tenant(&tenant.pubkey).await?;
        Ok(())
    }

    async fn make_invoice(&self, amount: i64) -> Result<String> {
        if self.platform_nwc_url.trim().is_empty() {
            return Err(anyhow!("NWC_URL is required to generate invoices"));
        }

        let uri = NostrWalletConnectURI::parse(&self.platform_nwc_url)?;
        let request = nip47::Request::make_invoice(MakeInvoiceRequest {
            amount: (amount as u64) * 1_000,
            description: Some("Relay hosting".to_string()),
            description_hash: None,
            expiry: None,
        });
        let response = self.send_nwc_request(&uri, request).await?;
        Ok(response.to_make_invoice()?.invoice)
    }

    async fn pay_invoice(&self, tenant_nwc_url: &str, invoice: &str) -> Result<()> {
        let uri = NostrWalletConnectURI::parse(tenant_nwc_url)?;
        let request = nip47::Request::pay_invoice(PayInvoiceRequest::new(invoice));
        self.send_nwc_request(&uri, request).await?.to_pay_invoice()?;
        Ok(())
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
            .max_by_key(|event| event.created_at)
            .ok_or_else(|| anyhow!("no NWC response"))?;

        Ok(nip47::Response::from_event(uri, &event)?)
    }

    async fn send_invoice_dm(
        &self,
        tenant: &Tenant,
        invoice: &NewInvoice,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
    ) -> Result<()> {
        let message = format!(
            "Invoice due: {} sats\nPeriod: {} - {}\nInvoice: {}",
            invoice.amount,
            period_start.to_rfc3339(),
            period_end.to_rfc3339(),
            invoice.invoice
        );
        self.notifier.send(&tenant.pubkey, &message).await
    }

    async fn send_payment_dm(&self, tenant: &Tenant, invoice: &NewInvoice) -> Result<()> {
        let message = format!(
            "Payment received: {} sats\nInvoice ID: {}",
            invoice.amount, invoice.id
        );
        self.notifier.send(&tenant.pubkey, &message).await
    }
}

fn next_billing_window(invoices: &[Invoice]) -> Result<(DateTime<Utc>, DateTime<Utc>, bool)> {
    let now = Utc::now();
    if invoices.is_empty() {
        let end = now + Months::new(1);
        return Ok((now, end, true));
    }

    let last = &invoices[0];
    if last.status == "pending" {
        return Ok((now, now, false));
    }

    let last_created = parse_timestamp(&last.created_at)?;
    let next_due = last_created + Months::new(1);
    if now < next_due {
        return Ok((now, next_due, false));
    }

    Ok((last_created, next_due, true))
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|e| anyhow!("invalid timestamp {value}: {e}"))?;
    Ok(parsed.with_timezone(&Utc))
}

fn build_invoice_items(
    invoice_id: &str,
    relays: &[Relay],
    period_start: DateTime<Utc>,
    period_end: DateTime<Utc>,
) -> Vec<NewInvoiceItem> {
    relays
        .iter()
        .filter_map(|relay| {
            let amount = plan_amount(&relay.plan);
            if amount == 0 {
                return None;
            }
            Some(NewInvoiceItem {
                id: Uuid::new_v4().to_string(),
                invoice: invoice_id.to_string(),
                relay: relay.id.clone(),
                amount,
                period_start: period_start.to_rfc3339(),
                period_end: period_end.to_rfc3339(),
            })
        })
        .collect()
}

fn plan_amount(plan: &str) -> i64 {
    match plan {
        "basic" => 10_000,
        "growth" => 50_000,
        _ => 0,
    }
}
