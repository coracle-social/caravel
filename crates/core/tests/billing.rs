//! End-to-end tests of the billing model across several billing periods. Each
//! test replays a tenant's months by pinning the clock, acting, and reconciling
//! the way the hourly loop does, then checks the resulting invoices.
//!
//! Every expected amount is written out with its derivation rather than
//! recomputed through the proration code, so a test fails if that code changes.
//! Tenants anchored on 2026-01-07 bill in periods of 31, 28, 31, then 30 days.

mod common;

use anyhow::Result;
use caravel_core::billing::ledger::InvoiceMethod;
use caravel_core::plans::{PLAN_BASIC, PLAN_GROWTH};
use caravel_core::relays::RelayStatus;
use caravel_core::{hosting, testing};
use common::*;

#[tokio::test]
async fn a_mid_period_relay_pays_its_slice_then_renews_in_full() -> Result<()> {
    let core = core().await;

    travel(&core, day(2026, 1, 7));
    let tenant = create_tenant(&core).await?;
    create_relay(&core, &tenant, PLAN_BASIC).await?;
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![500]);

    // 16 of the period's 31 days remain: 500 * 16/31 = 258.06.
    travel(&core, day(2026, 1, 22));
    create_relay(&core, &tenant, PLAN_BASIC).await?;
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![500, 258]);

    // Both relays were active when the period turned, so both renew in full.
    travel(&core, day(2026, 2, 7));
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![500, 258, 1000]);

    // Mid-period, nothing is due.
    travel(&core, day(2026, 2, 20));
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![500, 258, 1000]);

    // However many times billing runs at the turn, it renews once.
    travel(&core, day(2026, 3, 7));
    for _ in 0..3 {
        reconcile_and_pay(&core, &tenant).await?;
    }
    assert_eq!(
        invoice_amounts(&core, &tenant).await?,
        vec![500, 258, 1000, 1000]
    );

    let starts: Vec<i64> = invoices(&core, &tenant)
        .await?
        .iter()
        .map(|i| i.period_start)
        .collect();
    assert_eq!(
        starts,
        vec![
            day(2026, 1, 7),
            day(2026, 1, 7),
            day(2026, 2, 7),
            day(2026, 3, 7)
        ]
    );

    Ok(())
}

#[tokio::test]
async fn proration_rounds_to_the_nearest_whole_day() -> Result<()> {
    let core = core().await;
    let hour = 60 * 60;

    travel(&core, day(2026, 1, 7));
    let tenant = create_tenant(&core).await?;
    create_relay(&core, &tenant, PLAN_BASIC).await?;
    reconcile_and_pay(&core, &tenant).await?;

    // 30 days 13 hours left rounds up to all 31, so a relay added in the
    // first half-day of a period pays the full month, not a few cents short.
    travel(&core, day(2026, 1, 7) + 11 * hour);
    create_relay(&core, &tenant, PLAN_BASIC).await?;
    reconcile_and_pay(&core, &tenant).await?;

    // 15 days 11 hours left rounds down to 15: 500 * 15/31 = 241.94.
    travel(&core, day(2026, 1, 22) + 13 * hour);
    create_relay(&core, &tenant, PLAN_BASIC).await?;
    reconcile_and_pay(&core, &tenant).await?;

    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![500, 500, 242]);

    Ok(())
}

#[tokio::test]
async fn an_upgrade_bills_the_difference_and_renews_at_the_new_price() -> Result<()> {
    let core = core().await;

    travel(&core, day(2026, 1, 7));
    let tenant = create_tenant(&core).await?;
    let relay = create_relay(&core, &tenant, PLAN_BASIC).await?;
    reconcile_and_pay(&core, &tenant).await?;

    // Growth for the remaining 16/31 less the Basic already paid for it:
    // 2500 * 16/31 = 1290.32 and 500 * 16/31 = 258.06, so 1290 - 258.
    travel(&core, day(2026, 1, 22));
    change_plan(&core, &relay.id, PLAN_GROWTH).await?;
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![500, 1032]);

    let upgrade = &invoices(&core, &tenant).await?[1];
    assert_eq!(
        line_items(&core, upgrade).await?,
        vec![("Plan changed from Basic to Growth".to_string(), 1032)]
    );

    travel(&core, day(2026, 2, 7));
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(
        invoice_amounts(&core, &tenant).await?,
        vec![500, 1032, 2500]
    );

    Ok(())
}

#[tokio::test]
async fn a_downgrade_credit_carries_forward_until_it_is_spent() -> Result<()> {
    let core = core().await;

    travel(&core, day(2026, 1, 7));
    let tenant = create_tenant(&core).await?;
    let relay = create_relay(&core, &tenant, PLAN_GROWTH).await?;
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![2500]);

    // A downgrade credits 258 - 1290, and a negative balance cuts no invoice.
    travel(&core, day(2026, 1, 22));
    change_plan(&core, &relay.id, PLAN_BASIC).await?;
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(outstanding(&core, &tenant).await?, -1032);

    // Each Basic renewal draws the credit down, still cutting no invoice.
    travel(&core, day(2026, 2, 7));
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(outstanding(&core, &tenant).await?, -532);

    travel(&core, day(2026, 3, 7));
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(outstanding(&core, &tenant).await?, -32);
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![2500]);

    // The third renewal exceeds the remaining credit: 500 - 32.
    travel(&core, day(2026, 4, 7));
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![2500, 468]);
    assert_eq!(outstanding(&core, &tenant).await?, 0);

    let settled = &invoices(&core, &tenant).await?[1];
    assert_eq!(
        line_items(&core, settled).await?,
        vec![
            ("Plan changed from Growth to Basic".to_string(), -1032),
            ("Subscription renewal".to_string(), 500),
            ("Subscription renewal".to_string(), 500),
            ("Subscription renewal".to_string(), 500),
        ]
    );

    Ok(())
}

#[tokio::test]
async fn a_deactivated_relay_is_credited_and_stops_renewing() -> Result<()> {
    let core = core().await;

    travel(&core, day(2026, 1, 7));
    let tenant = create_tenant(&core).await?;
    let relay = create_relay(&core, &tenant, PLAN_GROWTH).await?;
    reconcile_and_pay(&core, &tenant).await?;

    // The unused 16/31 comes back: 2500 * 16/31 = 1290.32.
    travel(&core, day(2026, 1, 22));
    deactivate_relay(&core, &relay.id).await?;
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(outstanding(&core, &tenant).await?, -1290);

    // Inactive at both turns, so it renews at neither.
    for turn in [day(2026, 2, 7), day(2026, 3, 7)] {
        travel(&core, turn);
        reconcile_and_pay(&core, &tenant).await?;
        assert_eq!(outstanding(&core, &tenant).await?, -1290);
    }
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![2500]);

    // Back on with 28 of 31 days left: 2500 * 28/31 = 2258.06, less the
    // credit still on the ledger.
    travel(&core, day(2026, 3, 10));
    activate_relay(&core, &relay.id).await?;
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![2500, 968]);

    let returned = &invoices(&core, &tenant).await?[1];
    assert_eq!(
        line_items(&core, returned).await?,
        vec![
            ("Relay deactivated".to_string(), -1290),
            ("Relay reactivated".to_string(), 2258),
        ]
    );

    Ok(())
}

// Deleting a tenant settles their billing as a ban does, but doesn't keep them out.
#[tokio::test]
async fn a_deleted_tenant_is_forgiven_and_may_come_back() -> Result<()> {
    let core = core().await;

    travel(&core, day(2026, 1, 7));
    let tenant = create_tenant(&core).await?;
    let relay = create_relay(&core, &tenant, PLAN_GROWTH).await?;
    reconcile(&core, &tenant).await?;
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![2500]);

    travel(&core, day(2026, 1, 22));
    hosting::delete_tenant(&core, &tenant).await?;
    assert!(get_relay(&core, &relay.id).await?.is_purged());
    assert_eq!(get_tenant(&core, &tenant).await?.banned_at, None);
    assert!(
        invoices(&core, &tenant)
            .await?
            .iter()
            .all(|invoice| invoice.voided_at.is_some())
    );

    // The purge's credit for the unused 16/31 waits on the ledger, as after a
    // ban: 2500 * 16/31 = 1290.32.
    reconcile(&core, &tenant).await?;
    assert_eq!(outstanding(&core, &tenant).await?, -1290);

    create_relay(&core, &tenant, "free").await?;
    Ok(())
}

#[tokio::test]
async fn overage_is_billed_in_arrears_at_the_tier_the_period_closed_on() -> Result<()> {
    let core = core().await;

    travel(&core, day(2026, 1, 7));
    let tenant = create_tenant(&core).await?;
    let relay = create_relay(&core, &tenant, PLAN_BASIC).await?;
    reconcile_and_pay(&core, &tenant).await?;

    // Storage is a level and bills its peak; the A/V clock only climbs and
    // bills the participant hours it covered.
    travel(&core, day(2026, 1, 10));
    record_usage(&core, &relay, 0.8, 0.0).await?;
    travel(&core, day(2026, 1, 20));
    record_usage(&core, &relay, 3.2, 20.0).await?;
    travel(&core, day(2026, 2, 1));
    record_usage(&core, &relay, 2.1, 40.0).await?;

    // Nothing is metered until the period closes.
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![500]);

    // At Basic's rates: a 3.2 GB peak is 4 GB, 3 past the 1 included, at
    // $2 each; 40 participant hours is 10 past the 30 included, at $0.10
    // each. Billed
    // on the same invoice as the renewal for the month ahead.
    travel(&core, day(2026, 2, 7));
    for _ in 0..3 {
        reconcile_and_pay(&core, &tenant).await?;
    }
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

    // Upgrade mid-period: 25 of 28 days left, so 2500 * 25/28 = 2232.14
    // less 500 * 25/28 = 446.43.
    travel(&core, day(2026, 2, 10));
    change_plan(&core, &relay.id, PLAN_GROWTH).await?;
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(
        invoice_amounts(&core, &tenant).await?,
        vec![500, 1200, 1786]
    );

    // The clock is measured from the last reading before the period (40
    // participant hours on Feb 1), so this period covered 440 - 40 = 400.
    travel(&core, day(2026, 2, 15));
    record_usage(&core, &relay, 12.5, 440.0).await?;

    // The period started on Basic but closed on Growth, so Growth's rates
    // and allowances apply: a 12.5 GB peak is 13 GB, 3 past the 10
    // included, at $0.75; 400 participant hours is 100 past the 300
    // included, at $0.04.
    travel(&core, day(2026, 3, 7));
    reconcile_and_pay(&core, &tenant).await?;
    assert_eq!(
        invoice_amounts(&core, &tenant).await?,
        vec![500, 1200, 1786, 3125]
    );

    let closed_on_growth = &invoices(&core, &tenant).await?[3];
    assert_eq!(
        line_items(&core, closed_on_growth).await?,
        vec![
            (
                "A/V call overage (100 participant hours past the 300 included)".to_string(),
                400
            ),
            (
                "Storage overage (3 GB past the 10 included)".to_string(),
                225
            ),
            ("Subscription renewal".to_string(), 2500),
        ]
    );

    Ok(())
}

#[tokio::test]
async fn an_unpaid_invoice_churns_at_the_end_of_grace_and_a_return_forgives_it() -> Result<()> {
    let core = core().await;

    travel(&core, day(2026, 1, 7));
    let tenant = create_tenant(&core).await?;
    let first = create_relay(&core, &tenant, PLAN_BASIC).await?;
    reconcile(&core, &tenant).await?;
    let unpaid = invoices(&core, &tenant).await?[0].id.clone();

    // A second short of the 7-day grace period, the tenant is left alone.
    travel(&core, day(2026, 1, 14) - 1);
    reconcile(&core, &tenant).await?;
    assert_eq!(get_tenant(&core, &tenant).await?.churned_at, None);
    assert_eq!(
        get_relay(&core, &first.id).await?.status,
        RelayStatus::Active
    );

    // At the end of grace, the relays pause and the balance is forgiven.
    travel(&core, day(2026, 1, 14));
    reconcile(&core, &tenant).await?;
    assert_eq!(
        get_tenant(&core, &tenant).await?.churned_at,
        Some(day(2026, 1, 14))
    );
    assert_eq!(
        get_relay(&core, &first.id).await?.status,
        RelayStatus::Delinquent
    );
    assert!(invoices(&core, &tenant).await?[0].voided_at.is_some());

    // A delinquent relay isn't renewed.
    travel(&core, day(2026, 2, 7));
    reconcile(&core, &tenant).await?;
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![500]);

    // Coming back restores the paused relay alongside the new one, each
    // charged for 25 of 28 days: 500 * 25/28 = 446.43. The forgiven
    // invoice stays void.
    travel(&core, day(2026, 2, 10));
    create_relay(&core, &tenant, PLAN_BASIC).await?;
    reconcile(&core, &tenant).await?;

    assert_eq!(get_tenant(&core, &tenant).await?.churned_at, None);
    assert_eq!(
        get_relay(&core, &first.id).await?.status,
        RelayStatus::Active
    );
    assert_eq!(invoice_amounts(&core, &tenant).await?, vec![500, 892]);

    let forgiven = invoices(&core, &tenant)
        .await?
        .into_iter()
        .find(|i| i.id == unpaid);
    let forgiven = forgiven.expect("the unpaid invoice is kept");
    assert!(forgiven.voided_at.is_some() && forgiven.paid_at.is_none());

    let comeback = &invoices(&core, &tenant).await?[1];
    assert_eq!(
        line_items(&core, comeback).await?,
        vec![
            ("New relay created".to_string(), 446),
            ("Relay reactivated".to_string(), 446),
        ]
    );

    Ok(())
}

#[tokio::test]
async fn a_31st_anchor_renews_once_a_month_through_short_months() -> Result<()> {
    let core = core().await;

    travel(&core, day(2026, 1, 31));
    let tenant = create_tenant(&core).await?;
    create_relay(&core, &tenant, PLAN_BASIC).await?;
    reconcile_and_pay(&core, &tenant).await?;

    // Month-adds from the 31st clamp to the last day of short months. Billing
    // runs the day before and the day of each turn, and renews only on the turn.
    for (eve, turn) in [
        (day(2026, 2, 27), day(2026, 2, 28)),
        (day(2026, 3, 30), day(2026, 3, 31)),
        (day(2026, 4, 29), day(2026, 4, 30)),
    ] {
        travel(&core, eve);
        reconcile_and_pay(&core, &tenant).await?;
        travel(&core, turn);
        reconcile_and_pay(&core, &tenant).await?;
    }

    let periods: Vec<(i64, i64)> = invoices(&core, &tenant)
        .await?
        .iter()
        .map(|i| (i.period_start, i.period_end))
        .collect();

    // There are four periods, and each ends where the next begins.
    assert_eq!(
        periods,
        vec![
            (day(2026, 1, 31), day(2026, 2, 28)),
            (day(2026, 2, 28), day(2026, 3, 31)),
            (day(2026, 3, 31), day(2026, 4, 30)),
            (day(2026, 4, 30), day(2026, 5, 31)),
        ]
    );
    assert_eq!(
        invoice_amounts(&core, &tenant).await?,
        vec![500, 500, 500, 500]
    );

    Ok(())
}

#[tokio::test]
async fn operators_are_invoiced_but_never_collected_or_churned() -> Result<()> {
    let core = core().await;
    let operator = admin_pubkey(&core);

    travel(&core, day(2026, 1, 7));
    testing::insert_tenant(&core, &operator).await;
    let relay = create_relay(&core, &operator, PLAN_BASIC).await?;
    reconcile(&core, &operator).await?;

    travel(&core, day(2026, 2, 7));
    reconcile(&core, &operator).await?;

    // Past the grace period, the operator still owes nothing.
    travel(&core, day(2026, 2, 28));
    reconcile(&core, &operator).await?;

    assert_eq!(invoice_amounts(&core, &operator).await?, vec![500, 500]);
    for invoice in invoices(&core, &operator).await? {
        assert!(invoice.paid_at.is_some());
        assert_eq!(invoice.method, Some(InvoiceMethod::Comp));
    }
    assert_eq!(get_tenant(&core, &operator).await?.churned_at, None);
    assert_eq!(
        get_relay(&core, &relay.id).await?.status,
        RelayStatus::Active
    );

    Ok(())
}
