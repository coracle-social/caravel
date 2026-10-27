//! Billing runs concurrently with itself (the hourly loop, a tenant's reconcile
//! button, an admin's), so every write it makes must happen once however many
//! passes race. These tests race passes against one tenant.

mod common;

use anyhow::Result;
use caravel_core::billing::Collection;
use caravel_core::models::activity::ActivityType;
use caravel_core::models::plans::PLAN_BASIC;
use caravel_core::models::relays::RelayStatus;
use caravel_core::{billing, queries};
use common::*;
use futures_util::future::try_join_all;

const RACERS: usize = 8;

async fn race_reconciles(core: &caravel_core::Core, tenant_pubkey: &str) -> Result<()> {
    let tenant = get_tenant(core, tenant_pubkey).await?;
    try_join_all(
        (0..RACERS).map(|_| billing::reconcile_subscription(core, &tenant, Collection::Settle)),
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn racing_passes_bill_new_activity_once() -> Result<()> {
    let core = core().await;

    travel(&core, day(2026, 1, 7));
    let tenant = create_tenant(&core).await?;
    create_relay(&core, &tenant, PLAN_BASIC).await?;
    race_reconciles(&core, &tenant).await?;

    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![500]);
    assert_eq!(
        get_tenant(&core, &tenant).await?.billing_anchor,
        Some(day(2026, 1, 7))
    );
    assert_eq!(outstanding(&core, &tenant).await?, 0);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn racing_passes_renew_once_and_invoice_once() -> Result<()> {
    let core = core().await;

    travel(&core, day(2026, 1, 7));
    let tenant = create_tenant(&core).await?;
    create_relay(&core, &tenant, PLAN_BASIC).await?;
    create_relay(&core, &tenant, PLAN_BASIC).await?;
    reconcile_and_pay(&core, &tenant).await?;

    travel(&core, day(2026, 2, 7));
    race_reconciles(&core, &tenant).await?;

    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![1000, 1000]);
    let renewal = &invoices(&core, &tenant).await?[1];
    assert_eq!(
        line_items(&core, renewal).await?,
        vec![
            ("Subscription renewal".to_string(), 500),
            ("Subscription renewal".to_string(), 500),
        ]
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn racing_passes_bill_overage_once() -> Result<()> {
    let core = core().await;

    travel(&core, day(2026, 1, 7));
    let tenant = create_tenant(&core).await?;
    let relay = create_relay(&core, &tenant, PLAN_BASIC).await?;
    reconcile_and_pay(&core, &tenant).await?;

    travel(&core, day(2026, 1, 20));
    record_usage(&core, &relay, 3.2, 0.0).await?;

    travel(&core, day(2026, 2, 7));
    race_reconciles(&core, &tenant).await?;

    // 3 GB past the 1 included at $2, on the renewal's invoice.
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![500, 1100]);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn racing_passes_churn_once() -> Result<()> {
    let core = core().await;

    travel(&core, day(2026, 1, 7));
    let tenant = create_tenant(&core).await?;
    let relay = create_relay(&core, &tenant, PLAN_BASIC).await?;
    reconcile(&core, &tenant).await?;

    travel(&core, day(2026, 1, 14));
    race_reconciles(&core, &tenant).await?;

    assert_eq!(
        get_tenant(&core, &tenant).await?.churned_at,
        Some(day(2026, 1, 14))
    );
    assert_eq!(
        get_relay(&core, &relay.id).await?.status,
        RelayStatus::Delinquent
    );
    let paused = queries::activity::list_activity_for_relay(core.db.pool(), &relay.id)
        .await?
        .into_iter()
        .filter(|activity| activity.activity_type == ActivityType::MarkRelayDelinquent)
        .count();
    assert_eq!(paused, 1);
    assert!(invoices(&core, &tenant).await?[0].voided_at.is_some());

    Ok(())
}
