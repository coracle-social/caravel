//! Helpers for end-to-end tests of the core. Each test gets its own core and
//! clock on its test binary's shared database, and drives the same operations the
//! API and the workers do, against real SQL; only time is faked.

#![allow(dead_code)]

pub mod stripe;

use anyhow::{Result, anyhow};
use caravel_core::billing::Collection;
use caravel_core::crypto::generate_keypair;
use caravel_core::flag::Flag;
use caravel_core::models::ledger;
use caravel_core::models::ledger::{Invoice, InvoiceItem};
use caravel_core::models::plans::{GB, SECONDS_PER_HOUR};
use caravel_core::models::relays::{Relay, RelaySettings};
use caravel_core::models::tenants::Tenant;
use caravel_core::models::usage::UsageSample;
use caravel_core::{Core, billing, commands, hosting, queries, testing};
use chrono::{NaiveDate, TimeZone, Utc};

/// A core whose admin is a fresh pubkey, with the clock released.
pub async fn core() -> Core {
    let (admin, _) = generate_keypair();
    testing::core(&admin).await
}

// --- Time ---

/// Midnight UTC on the given day, as a unix timestamp.
pub fn day(year: i32, month: u32, day: u32) -> i64 {
    let date = NaiveDate::from_ymd_opt(year, month, day).expect("invalid date");
    Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).expect("invalid time"))
        .timestamp()
}

/// Move the core's clock to `at`. Everything written afterwards is stamped then.
pub fn travel(core: &Core, at: i64) {
    core.clock.pin(at);
}

// --- Actors ---

/// The pubkey the core's config names as admin, whose invoices are comped.
pub fn admin_pubkey(core: &Core) -> String {
    core.config.admin_pubkeys[0].clone()
}

/// Sign up a tenant with a fresh keypair, returning its pubkey.
pub async fn create_tenant(core: &Core) -> Result<String> {
    let (pubkey, _) = generate_keypair();
    testing::insert_tenant(core, &pubkey).await;
    Ok(pubkey)
}

/// Create a relay on `plan_id`, with every metered feature on for paid plans.
pub async fn create_relay(core: &Core, tenant_pubkey: &str, plan_id: &str) -> Result<Relay> {
    let metered = Some(Flag(plan_id != "free"));
    Ok(hosting::create_relay(
        core,
        tenant_pubkey,
        RelaySettings {
            subdomain: Some(testing::subdomain()),
            domain: Some(testing::RELAY_DOMAIN.to_string()),
            plan_id: Some(plan_id.to_string()),
            blossom_enabled: metered,
            livekit_enabled: metered,
            ..Default::default()
        },
    )
    .await?)
}

pub async fn change_plan(core: &Core, relay_id: &str, plan_id: &str) -> Result<Relay> {
    let settings = RelaySettings {
        plan_id: Some(plan_id.to_string()),
        ..Default::default()
    };
    Ok(hosting::update_relay(core, get_relay(core, relay_id).await?, settings).await?)
}

pub async fn deactivate_relay(core: &Core, relay_id: &str) -> Result<()> {
    Ok(hosting::deactivate_relay(core, &get_relay(core, relay_id).await?).await?)
}

pub async fn activate_relay(core: &Core, relay_id: &str) -> Result<()> {
    Ok(hosting::reactivate_relay(core, &get_relay(core, relay_id).await?).await?)
}

/// Record a usage reading for a relay, as the hourly sampler would, stamped now.
pub async fn record_usage(
    core: &Core,
    relay: &Relay,
    storage_gb: f64,
    participant_hours: f64,
) -> Result<()> {
    commands::usage::insert_usage_sample(
        core.db.pool(),
        &UsageSample {
            id: uuid::Uuid::new_v4().to_string(),
            relay_id: relay.id.clone(),
            tenant_pubkey: relay.tenant_pubkey.clone(),
            storage_bytes: (storage_gb * GB as f64) as i64,
            participant_seconds: (participant_hours * SECONDS_PER_HOUR as f64) as i64,
            created_at: core.clock.now(),
        },
    )
    .await?;
    Ok(())
}

/// One pass of billing for a tenant, as the hourly loop runs it but without
/// collecting payment, so every invoice it cuts stays open. An open invoice churns
/// its tenant seven days later; scenarios not about dunning use
/// [`reconcile_and_pay`].
pub async fn reconcile(core: &Core, tenant_pubkey: &str) -> Result<()> {
    let tenant = get_tenant(core, tenant_pubkey).await?;
    Ok(billing::reconcile_subscription(core, &tenant, Collection::Settle).await?)
}

/// One pass of billing for a tenant in good standing, who pays what it invoices.
pub async fn reconcile_and_pay(core: &Core, tenant_pubkey: &str) -> Result<()> {
    reconcile(core, tenant_pubkey).await?;
    pay_invoices(core, tenant_pubkey).await
}

/// Settle every open invoice the way a tenant paying a Lightning invoice by hand
/// does: a bolt11 on the invoice, settled out of band.
pub async fn pay_invoices(core: &Core, tenant_pubkey: &str) -> Result<()> {
    let now = core.clock.now();
    for invoice in
        queries::ledger::list_open_invoices_for_tenant(core.db.pool(), tenant_pubkey).await?
    {
        let lnbc = format!("lnbc-e2e-{}", invoice.id);
        let bolt11 =
            commands::ledger::insert_bolt11(core.db.pool(), &invoice.id, &lnbc, 0, now + 3600, now)
                .await?;
        commands::ledger::mark_bolt11_settled(core.db.pool(), &bolt11.id, now).await?;
        commands::ledger::mark_invoice_paid(
            core.db.pool(),
            &invoice.id,
            ledger::InvoiceMethod::Oob,
            now,
        )
        .await?;
    }
    Ok(())
}

// --- Reads ---

pub async fn get_tenant(core: &Core, pubkey: &str) -> Result<Tenant> {
    queries::tenants::get_tenant(core.db.pool(), pubkey)
        .await?
        .ok_or_else(|| anyhow!("no tenant {pubkey}"))
}

pub async fn get_relay(core: &Core, relay_id: &str) -> Result<Relay> {
    queries::relays::get_relay(core.db.pool(), relay_id)
        .await?
        .ok_or_else(|| anyhow!("no relay {relay_id}"))
}

/// A tenant's invoices, oldest first.
pub async fn invoices(core: &Core, tenant_pubkey: &str) -> Result<Vec<Invoice>> {
    let mut invoices =
        queries::ledger::list_invoices_for_tenant(core.db.pool(), tenant_pubkey).await?;
    invoices.reverse();
    Ok(invoices)
}

/// Each invoice's amount, oldest first.
pub async fn invoice_amounts(core: &Core, tenant_pubkey: &str) -> Result<Vec<i64>> {
    Ok(invoices(core, tenant_pubkey)
        .await?
        .iter()
        .map(|invoice| invoice.amount)
        .collect())
}

/// An invoice's line items as `(description, amount)`, sorted so items stamped in
/// the same second compare deterministically.
pub async fn line_items(core: &Core, invoice: &Invoice) -> Result<Vec<(String, i64)>> {
    let mut items: Vec<(String, i64)> =
        queries::ledger::list_invoice_items_for_invoice(core.db.pool(), &invoice.id)
            .await?
            .into_iter()
            .map(|item: InvoiceItem| (item.description, item.amount))
            .collect();
    items.sort();
    Ok(items)
}

/// The sum of the tenant's line items no invoice has claimed: credits carried
/// forward, or charges below the minimum an invoice is cut for.
pub async fn outstanding(core: &Core, tenant_pubkey: &str) -> Result<i64> {
    Ok(
        queries::ledger::list_outstanding_invoice_items_for_tenant(core.db.pool(), tenant_pubkey)
            .await?
            .iter()
            .map(|item| item.amount)
            .sum(),
    )
}
