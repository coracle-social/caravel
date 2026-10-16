//! Overage billed from usage the platform measures itself: the blobs a relay holds
//! and the time spent in its calls, read by the hourly sampler. The sampler reads
//! every relay, so this binary holds one test.

mod common;

use anyhow::Result;
use caravel_core::media;
use caravel_core::plans::{GB, PLAN_BASIC, SECONDS_PER_HOUR};
use caravel_core::{Core, calls, usage};
use common::*;

async fn upload(core: &Core, relay_id: &str, sha256: &str, size: i64, pubkey: &str) -> Result<()> {
    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            media::record_upload(conn, relay_id, sha256, "video/mp4", size, pubkey, now).await
        })
        .await?;
    Ok(())
}

async fn delete(core: &Core, relay_id: &str, sha256: &str, pubkey: &str) -> Result<()> {
    core.db
        .with_tx(async |conn| media::delete_upload(conn, relay_id, sha256, pubkey).await)
        .await?;
    Ok(())
}

async fn call(core: &Core, relay_id: &str, sid: &str, hours: i64) -> Result<()> {
    let now = core.clock.now();
    calls::record_session(
        core.db.pool(),
        relay_id,
        sid,
        "room",
        hours * SECONDS_PER_HOUR,
        now,
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn overage_is_billed_from_measured_media_and_calls() -> Result<()> {
    let core = core().await;

    travel(&core, day(2026, 1, 7));
    let tenant = create_tenant(&core).await?;
    let relay = create_relay(&core, &tenant, PLAN_BASIC).await?;
    let free = create_relay(&core, &tenant, "free").await?;
    reconcile_and_pay(&core, &tenant).await?;

    // Two members uploading the same file store it once.
    travel(&core, day(2026, 1, 10));
    upload(&core, &relay.id, "a", 8 * GB / 10, "alice").await?;
    upload(&core, &relay.id, "a", 8 * GB / 10, "bob").await?;
    upload(&core, &free.id, "a", 8 * GB / 10, "alice").await?;
    usage::sample_usage(&core).await?;

    travel(&core, day(2026, 1, 20));
    upload(&core, &relay.id, "b", 24 * GB / 10, "alice").await?;
    call(&core, &relay.id, "PA_1", 20).await?;
    usage::sample_usage(&core).await?;

    // Storage falls back once the large file is gone; the call clock only climbs.
    travel(&core, day(2026, 2, 1));
    delete(&core, &relay.id, "b", "alice").await?;
    call(&core, &relay.id, "PA_2", 20).await?;
    usage::sample_usage(&core).await?;

    // A 3.2 GB peak is 4 GB, 3 past Basic's 1 included, at $2 each; 40
    // participant hours is 10 past the 30 included, at $0.10 each.
    travel(&core, day(2026, 2, 7));
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![500, 1200]);

    let arrears = &invoices(&core, &tenant).await?[1];
    assert_eq!(
        line_items(&core, arrears).await?,
        vec![
            (
                "A/V call overage (10 participant hours past the 30 included)".to_string(),
                100
            ),
            (
                "Storage overage (3 GB past the 1 included)".to_string(),
                600
            ),
            ("Subscription renewal".to_string(), 500),
        ]
    );

    // A relay on the free plan isn't metered.
    let totals = usage::get_usage_totals_for_tenant(
        core.db.pool(),
        &tenant,
        day(2026, 1, 7),
        day(2026, 2, 7),
    )
    .await?;
    assert_eq!(totals.len(), 1);
    assert_eq!(totals[0].relay_id, relay.id);
    Ok(())
}
