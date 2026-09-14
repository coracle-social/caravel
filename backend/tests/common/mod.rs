//! Harness for the backend's end-to-end tests: one tokio runtime, one migrated
//! SQLite database, and one pinned clock, shared by every test in a binary and
//! handed out one test at a time. Tests drive the same `command` writes the
//! routes make and the same `Billing` reconcile the hourly loop runs, against
//! real SQL; only time is faked.

#![allow(dead_code)]

use std::future::Future;
use std::sync::{Mutex, Once, OnceLock};

use anyhow::{Result, anyhow};
use backend::billing::Billing;
use backend::models::{GB, Invoice, InvoiceItem, Relay, SECONDS_PER_HOUR, Tenant, UsageSample};
use backend::robot::Robot;
use backend::{clock, command, db, env, query};
use chrono::{NaiveDate, TimeZone, Utc};
use nostr_sdk::Keys;
use tokio::runtime::Runtime;

// The pool and env are process-wide `OnceLock`s, and sqlx binds a pool's
// connections to the runtime that opened them, so every test shares this one
// runtime rather than getting its own from `#[tokio::test]`.
static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static SETUP: Once = Once::new();

// The clock is process-wide too, so tests take turns with it.
static SERIAL: Mutex<()> = Mutex::new(());

static ADMIN: OnceLock<Keys> = OnceLock::new();

/// Run one end-to-end test with the shared database and a clean clock.
pub fn run<F, Fut>(test: F)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<()>>,
{
    // A test that panicked still hands the turn on.
    let _turn = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let runtime = RUNTIME.get_or_init(|| Runtime::new().expect("failed to build runtime"));

    SETUP.call_once(|| {
        configure_env();
        env::init();
        runtime
            .block_on(db::init())
            .expect("failed to init database");
    });

    // A test that panicked mid-way may have left the clock pinned.
    clock::release();
    runtime.block_on(test()).expect("test failed");
}

fn configure_env() {
    let db_path = format!("{}/billing-e2e.db", env!("CARGO_TARGET_TMPDIR"));
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{db_path}{suffix}"));
    }

    let robot = Keys::generate();
    let wallet = Keys::generate();
    let admin = ADMIN.get_or_init(Keys::generate);

    // Every relay and API URL points at a closed port, so any network call
    // fails immediately.
    let dead_relay = "ws://127.0.0.1:1";
    let vars = [
        ("SERVER_URL", "http://localhost".to_string()),
        ("SERVER_PORT", "0".to_string()),
        ("SERVER_ADMIN_PUBKEYS", admin.public_key().to_hex()),
        ("SERVER_ALLOW_ORIGINS", "http://localhost".to_string()),
        ("DATABASE_URL", format!("sqlite://{db_path}")),
        ("ROBOT_SECRET", robot.secret_key().to_secret_hex()),
        (
            "ROBOT_WALLET",
            format!(
                "nostr+walletconnect://{}?relay={dead_relay}&secret={}",
                wallet.public_key().to_hex(),
                wallet.secret_key().to_secret_hex()
            ),
        ),
        ("ROBOT_NAME", "caravel-e2e".to_string()),
        ("ROBOT_PICTURE", "http://localhost/robot.png".to_string()),
        ("ROBOT_DESCRIPTION", "e2e".to_string()),
        ("ROBOT_OUTBOX_RELAYS", dead_relay.to_string()),
        ("ROBOT_INDEXER_RELAYS", dead_relay.to_string()),
        ("ROBOT_MESSAGING_RELAYS", dead_relay.to_string()),
        ("BLOSSOM_S3_REGION", "none".to_string()),
        ("BLOSSOM_S3_BUCKET", "none".to_string()),
        ("BLOSSOM_S3_ENDPOINT", "http://127.0.0.1:1".to_string()),
        ("BLOSSOM_S3_ACCESS_KEY", "none".to_string()),
        ("BLOSSOM_S3_SECRET_KEY", "none".to_string()),
        ("ZOOID_API_URL", "http://127.0.0.1:1".to_string()),
        ("ZOOID_DOMAINS", "relay.test".to_string()),
        ("LIVEKIT_URL", dead_relay.to_string()),
        ("LIVEKIT_API_KEY", "none".to_string()),
        ("LIVEKIT_API_SECRET", "none".to_string()),
        ("STRIPE_SECRET_KEY", "sk_test_none".to_string()),
    ];

    for (key, value) in vars {
        // SAFETY: runs once, before `env::init` or anything else reads the
        // environment, while the serial lock holds every other test off.
        unsafe { std::env::set_var(key, value) };
    }
}

// --- Time ---

/// Midnight UTC on the given day, as a unix timestamp.
pub fn day(year: i32, month: u32, day: u32) -> i64 {
    let date = NaiveDate::from_ymd_opt(year, month, day).expect("invalid date");
    Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).expect("invalid time"))
        .timestamp()
}

/// Move the pinned clock to `at`. Everything written afterwards is stamped then.
pub fn travel(at: i64) {
    clock::pin(at);
}

// --- Actors ---

pub fn billing() -> Billing {
    Billing::new(Robot::new())
}

/// The pubkey `SERVER_ADMIN_PUBKEYS` names, whose invoices are comped.
pub fn admin_pubkey() -> String {
    ADMIN
        .get()
        .expect("harness not set up")
        .public_key()
        .to_hex()
}

/// Sign up a tenant with a fresh keypair, returning its pubkey.
pub async fn create_tenant() -> Result<String> {
    let pubkey = Keys::generate().public_key().to_hex();
    create_tenant_with(&pubkey).await?;
    Ok(pubkey)
}

pub async fn create_tenant_with(pubkey: &str) -> Result<()> {
    command::create_tenant(&Tenant {
        pubkey: pubkey.to_string(),
        return_url: "http://localhost/account".to_string(),
        created_at: clock::now(),
        stripe_customer_id: format!("cus_{}", &pubkey[..12]),
        ..Default::default()
    })
    .await
}

/// Create a relay on `plan_id`, with every metered feature switched on so paid
/// plans have something to meter.
pub async fn create_relay(tenant_pubkey: &str, plan_id: &str) -> Result<Relay> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let metered = i64::from(plan_id != "free");

    command::create_relay(&Relay {
        id: id.clone(),
        tenant_pubkey: tenant_pubkey.to_string(),
        subdomain: format!("r{}", &id[..12]),
        zooid_domain: "relay.test".to_string(),
        plan_id: plan_id.to_string(),
        blossom_enabled: metered,
        livekit_enabled: metered,
        ..Default::default()
    })
    .await?;

    get_relay(&id).await
}

pub async fn change_plan(relay_id: &str, plan_id: &str) -> Result<Relay> {
    let relay = Relay {
        plan_id: plan_id.to_string(),
        ..get_relay(relay_id).await?
    };
    command::update_relay(&relay).await?;
    get_relay(relay_id).await
}

pub async fn deactivate_relay(relay_id: &str) -> Result<()> {
    command::deactivate_relay(&get_relay(relay_id).await?).await
}

pub async fn activate_relay(relay_id: &str) -> Result<()> {
    command::activate_relay(&get_relay(relay_id).await?).await
}

/// Record a usage reading for a relay, as the hourly sampler would after
/// polling zooid, stamped at the current pinned time.
pub async fn record_usage(relay: &Relay, storage_gb: f64, participant_hours: f64) -> Result<()> {
    command::insert_usage_sample(&UsageSample {
        id: uuid::Uuid::new_v4().to_string(),
        relay_id: relay.id.clone(),
        tenant_pubkey: relay.tenant_pubkey.clone(),
        storage_bytes: (storage_gb * GB as f64) as i64,
        participant_seconds: (participant_hours * SECONDS_PER_HOUR as f64) as i64,
        created_at: clock::now(),
    })
    .await
}

/// One pass of billing for a tenant, as the hourly loop runs it but without
/// collecting payment, so nothing reaches Lightning or Stripe and every invoice
/// it cuts stays open. An open invoice churns its tenant seven days later, which
/// also voids the tenant's credits; scenarios not about dunning use
/// [`reconcile_and_pay`]. Re-reads the tenant first, since billing updates its
/// anchor and markers.
pub async fn reconcile(billing: &Billing, tenant_pubkey: &str) -> Result<()> {
    billing
        .reconcile_subscription(&get_tenant(tenant_pubkey).await?, false)
        .await
}

/// One pass of billing for a tenant in good standing, who pays whatever it
/// invoices.
pub async fn reconcile_and_pay(billing: &Billing, tenant_pubkey: &str) -> Result<()> {
    reconcile(billing, tenant_pubkey).await?;
    pay_invoices(tenant_pubkey).await
}

/// Settle every open invoice the way a tenant paying a Lightning invoice by hand
/// does: a bolt11 on the invoice, settled out of band.
pub async fn pay_invoices(tenant_pubkey: &str) -> Result<()> {
    for invoice in query::list_open_invoices(tenant_pubkey).await? {
        let lnbc = format!("lnbc-e2e-{}", invoice.id);
        let bolt11 = command::insert_bolt11(&invoice.id, &lnbc, 0, clock::now() + 3600)
            .await?
            .ok_or_else(|| anyhow!("no bolt11 for invoice {}", invoice.id))?;
        command::settle_invoice_out_of_band(&bolt11.id, &invoice.id).await?;
    }
    Ok(())
}

// --- Reads ---

pub async fn get_tenant(pubkey: &str) -> Result<Tenant> {
    query::get_tenant(pubkey)
        .await?
        .ok_or_else(|| anyhow!("no tenant {pubkey}"))
}

pub async fn get_relay(relay_id: &str) -> Result<Relay> {
    query::get_relay(relay_id)
        .await?
        .ok_or_else(|| anyhow!("no relay {relay_id}"))
}

/// A tenant's invoices, oldest first.
pub async fn invoices(tenant_pubkey: &str) -> Result<Vec<Invoice>> {
    let mut invoices = query::list_invoices_for_tenant(tenant_pubkey).await?;
    invoices.sort_by_key(|invoice| invoice.created_at);
    Ok(invoices)
}

/// Each invoice's amount, oldest first.
pub async fn invoice_amounts(tenant_pubkey: &str) -> Result<Vec<i64>> {
    Ok(invoices(tenant_pubkey)
        .await?
        .iter()
        .map(|invoice| invoice.amount)
        .collect())
}

/// An invoice's line items as `(description, amount)`, sorted so items stamped
/// in the same second — a closed period's overage and the next period's
/// renewal share one — compare deterministically.
pub async fn line_items(invoice: &Invoice) -> Result<Vec<(String, i64)>> {
    let mut items: Vec<(String, i64)> = query::list_invoice_items_for_invoice(&invoice.id)
        .await?
        .into_iter()
        .map(|item: InvoiceItem| (item.description, item.amount))
        .collect();
    items.sort();
    Ok(items)
}

/// The sum of the tenant's line items not yet claimed onto an invoice: credits
/// carried forward, or charges below the minimum an invoice is cut for.
pub async fn outstanding(tenant_pubkey: &str) -> Result<i64> {
    Ok(query::list_unbilled_invoice_items(tenant_pubkey)
        .await?
        .iter()
        .map(|item| item.amount)
        .sum())
}
