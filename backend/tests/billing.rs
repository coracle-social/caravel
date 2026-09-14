//! End-to-end tests of the billing model across several billing periods. Each
//! test replays a tenant's months by pinning the clock, acting, and reconciling
//! the way the hourly loop does, then checks the resulting invoices.
//!
//! Every expected amount is written out with its derivation rather than
//! recomputed through the proration code, so a test fails if that code changes.
//! Tenants anchored on 2026-01-07 bill in periods of 31, 28, 31, then 30 days.

mod common;

use backend::models::{PLAN_BASIC, PLAN_GROWTH, RELAY_STATUS_ACTIVE, RELAY_STATUS_DELINQUENT};
use common::*;

#[test]
fn a_mid_period_relay_pays_its_slice_then_renews_in_full() {
    run(|| async {
        let billing = billing();

        travel(day(2026, 1, 7));
        let tenant = create_tenant().await?;
        create_relay(&tenant, PLAN_BASIC).await?;
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(invoice_amounts(&tenant).await?, vec![500]);

        // 16 of the period's 31 days remain: 500 * 16/31 = 258.06.
        travel(day(2026, 1, 22));
        create_relay(&tenant, PLAN_BASIC).await?;
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(invoice_amounts(&tenant).await?, vec![500, 258]);

        // Both relays were active when the period turned, so both renew in full.
        travel(day(2026, 2, 7));
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(invoice_amounts(&tenant).await?, vec![500, 258, 1000]);

        // Mid-period, nothing is due.
        travel(day(2026, 2, 20));
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(invoice_amounts(&tenant).await?, vec![500, 258, 1000]);

        // However many times billing runs at the turn, it renews once.
        travel(day(2026, 3, 7));
        for _ in 0..3 {
            reconcile_and_pay(&billing, &tenant).await?;
        }
        assert_eq!(invoice_amounts(&tenant).await?, vec![500, 258, 1000, 1000]);

        let starts: Vec<i64> = invoices(&tenant)
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
    });
}

#[test]
fn proration_rounds_to_the_nearest_whole_day() {
    run(|| async {
        let billing = billing();
        let hour = 60 * 60;

        travel(day(2026, 1, 7));
        let tenant = create_tenant().await?;
        create_relay(&tenant, PLAN_BASIC).await?;
        reconcile_and_pay(&billing, &tenant).await?;

        // 30 days 13 hours left rounds up to all 31, so a relay added in the
        // first half-day of a period pays the full month, not a few cents short.
        travel(day(2026, 1, 7) + 11 * hour);
        create_relay(&tenant, PLAN_BASIC).await?;
        reconcile_and_pay(&billing, &tenant).await?;

        // 15 days 11 hours left rounds down to 15: 500 * 15/31 = 241.94.
        travel(day(2026, 1, 22) + 13 * hour);
        create_relay(&tenant, PLAN_BASIC).await?;
        reconcile_and_pay(&billing, &tenant).await?;

        assert_eq!(invoice_amounts(&tenant).await?, vec![500, 500, 242]);

        Ok(())
    });
}

#[test]
fn an_upgrade_bills_the_difference_and_renews_at_the_new_price() {
    run(|| async {
        let billing = billing();

        travel(day(2026, 1, 7));
        let tenant = create_tenant().await?;
        let relay = create_relay(&tenant, PLAN_BASIC).await?;
        reconcile_and_pay(&billing, &tenant).await?;

        // Growth for the remaining 16/31 less the Basic already paid for it:
        // 2500 * 16/31 = 1290.32 and 500 * 16/31 = 258.06, so 1290 - 258.
        travel(day(2026, 1, 22));
        change_plan(&relay.id, PLAN_GROWTH).await?;
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(invoice_amounts(&tenant).await?, vec![500, 1032]);

        let upgrade = &invoices(&tenant).await?[1];
        assert_eq!(
            line_items(upgrade).await?,
            vec![("Plan changed from Basic to Growth".to_string(), 1032)]
        );

        travel(day(2026, 2, 7));
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(invoice_amounts(&tenant).await?, vec![500, 1032, 2500]);

        Ok(())
    });
}

#[test]
fn a_downgrade_credit_carries_forward_until_it_is_spent() {
    run(|| async {
        let billing = billing();

        travel(day(2026, 1, 7));
        let tenant = create_tenant().await?;
        let relay = create_relay(&tenant, PLAN_GROWTH).await?;
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(invoice_amounts(&tenant).await?, vec![2500]);

        // A downgrade credits 258 - 1290, and a negative balance cuts no invoice.
        travel(day(2026, 1, 22));
        change_plan(&relay.id, PLAN_BASIC).await?;
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(outstanding(&tenant).await?, -1032);

        // Each Basic renewal draws the credit down, still cutting no invoice.
        travel(day(2026, 2, 7));
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(outstanding(&tenant).await?, -532);

        travel(day(2026, 3, 7));
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(outstanding(&tenant).await?, -32);
        assert_eq!(invoice_amounts(&tenant).await?, vec![2500]);

        // The third renewal exceeds the remaining credit: 500 - 32.
        travel(day(2026, 4, 7));
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(invoice_amounts(&tenant).await?, vec![2500, 468]);
        assert_eq!(outstanding(&tenant).await?, 0);

        let settled = &invoices(&tenant).await?[1];
        assert_eq!(
            line_items(settled).await?,
            vec![
                ("Plan changed from Growth to Basic".to_string(), -1032),
                ("Subscription renewal".to_string(), 500),
                ("Subscription renewal".to_string(), 500),
                ("Subscription renewal".to_string(), 500),
            ]
        );

        Ok(())
    });
}

#[test]
fn a_deactivated_relay_is_credited_and_stops_renewing() {
    run(|| async {
        let billing = billing();

        travel(day(2026, 1, 7));
        let tenant = create_tenant().await?;
        let relay = create_relay(&tenant, PLAN_GROWTH).await?;
        reconcile_and_pay(&billing, &tenant).await?;

        // The unused 16/31 comes back: 2500 * 16/31 = 1290.32.
        travel(day(2026, 1, 22));
        deactivate_relay(&relay.id).await?;
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(outstanding(&tenant).await?, -1290);

        // Inactive at both turns, so it renews at neither.
        for turn in [day(2026, 2, 7), day(2026, 3, 7)] {
            travel(turn);
            reconcile_and_pay(&billing, &tenant).await?;
            assert_eq!(outstanding(&tenant).await?, -1290);
        }
        assert_eq!(invoice_amounts(&tenant).await?, vec![2500]);

        // Back on with 28 of 31 days left: 2500 * 28/31 = 2258.06, less the
        // credit still on the ledger.
        travel(day(2026, 3, 10));
        activate_relay(&relay.id).await?;
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(invoice_amounts(&tenant).await?, vec![2500, 968]);

        let returned = &invoices(&tenant).await?[1];
        assert_eq!(
            line_items(returned).await?,
            vec![
                ("Relay deactivated".to_string(), -1290),
                ("Relay reactivated".to_string(), 2258),
            ]
        );

        Ok(())
    });
}

#[test]
fn overage_is_billed_in_arrears_at_the_tier_the_period_closed_on() {
    run(|| async {
        let billing = billing();

        travel(day(2026, 1, 7));
        let tenant = create_tenant().await?;
        let relay = create_relay(&tenant, PLAN_BASIC).await?;
        reconcile_and_pay(&billing, &tenant).await?;

        // Storage is a level and bills its peak; the A/V clock only climbs and
        // bills the participant hours it covered.
        travel(day(2026, 1, 10));
        record_usage(&relay, 0.8, 0.0).await?;
        travel(day(2026, 1, 20));
        record_usage(&relay, 3.2, 20.0).await?;
        travel(day(2026, 2, 1));
        record_usage(&relay, 2.1, 40.0).await?;

        // Nothing is metered until the period closes.
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(invoice_amounts(&tenant).await?, vec![500]);

        // At Basic's rates: a 3.2 GB peak is 4 GB, 3 past the 1 included, at
        // $2 each; 40 participant hours is 10 past the 30 included, at $0.10
        // each. Billed
        // on the same invoice as the renewal for the month ahead.
        travel(day(2026, 2, 7));
        for _ in 0..3 {
            reconcile_and_pay(&billing, &tenant).await?;
        }
        assert_eq!(invoice_amounts(&tenant).await?, vec![500, 1200]);

        let arrears = &invoices(&tenant).await?[1];
        assert_eq!(
            line_items(arrears).await?,
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
        travel(day(2026, 2, 10));
        change_plan(&relay.id, PLAN_GROWTH).await?;
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(invoice_amounts(&tenant).await?, vec![500, 1200, 1786]);

        // The clock is measured from the last reading before the period (40
        // participant hours on Feb 1), so this period covered 440 - 40 = 400.
        travel(day(2026, 2, 15));
        record_usage(&relay, 12.5, 440.0).await?;

        // The period started on Basic but closed on Growth, so Growth's rates
        // and allowances apply: a 12.5 GB peak is 13 GB, 3 past the 10
        // included, at $0.75; 400 participant hours is 100 past the 300
        // included, at $0.04.
        travel(day(2026, 3, 7));
        reconcile_and_pay(&billing, &tenant).await?;
        assert_eq!(invoice_amounts(&tenant).await?, vec![500, 1200, 1786, 3125]);

        let closed_on_growth = &invoices(&tenant).await?[3];
        assert_eq!(
            line_items(closed_on_growth).await?,
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
    });
}

#[test]
fn an_unpaid_invoice_churns_at_the_end_of_grace_and_a_return_forgives_it() {
    run(|| async {
        let billing = billing();

        travel(day(2026, 1, 7));
        let tenant = create_tenant().await?;
        let first = create_relay(&tenant, PLAN_BASIC).await?;
        reconcile(&billing, &tenant).await?;
        let unpaid = invoices(&tenant).await?[0].id.clone();

        // A second short of the 7-day grace period, the tenant is left alone.
        travel(day(2026, 1, 14) - 1);
        reconcile(&billing, &tenant).await?;
        assert_eq!(get_tenant(&tenant).await?.churned_at, None);
        assert_eq!(get_relay(&first.id).await?.status, RELAY_STATUS_ACTIVE);

        // At the end of grace, the relays pause and the balance is forgiven.
        travel(day(2026, 1, 14));
        reconcile(&billing, &tenant).await?;
        assert_eq!(
            get_tenant(&tenant).await?.churned_at,
            Some(day(2026, 1, 14))
        );
        assert_eq!(get_relay(&first.id).await?.status, RELAY_STATUS_DELINQUENT);
        assert!(invoices(&tenant).await?[0].voided_at.is_some());

        // A delinquent relay isn't renewed.
        travel(day(2026, 2, 7));
        reconcile(&billing, &tenant).await?;
        assert_eq!(invoice_amounts(&tenant).await?, vec![500]);

        // Coming back restores the paused relay alongside the new one, each
        // charged for 25 of 28 days: 500 * 25/28 = 446.43. The forgiven
        // invoice stays void.
        travel(day(2026, 2, 10));
        create_relay(&tenant, PLAN_BASIC).await?;
        reconcile(&billing, &tenant).await?;

        assert_eq!(get_tenant(&tenant).await?.churned_at, None);
        assert_eq!(get_relay(&first.id).await?.status, RELAY_STATUS_ACTIVE);
        assert_eq!(invoice_amounts(&tenant).await?, vec![500, 892]);

        let forgiven = invoices(&tenant)
            .await?
            .into_iter()
            .find(|i| i.id == unpaid);
        let forgiven = forgiven.expect("the unpaid invoice is kept");
        assert!(forgiven.voided_at.is_some() && forgiven.paid_at.is_none());

        let comeback = &invoices(&tenant).await?[1];
        assert_eq!(
            line_items(comeback).await?,
            vec![
                ("New relay created".to_string(), 446),
                ("Relay reactivated".to_string(), 446),
            ]
        );

        Ok(())
    });
}

#[test]
fn a_31st_anchor_renews_once_a_month_through_short_months() {
    run(|| async {
        let billing = billing();

        travel(day(2026, 1, 31));
        let tenant = create_tenant().await?;
        create_relay(&tenant, PLAN_BASIC).await?;
        reconcile_and_pay(&billing, &tenant).await?;

        // Month-adds from the 31st clamp to the last day of short months. Billing
        // runs the day before and the day of each turn, and renews only on the turn.
        for (eve, turn) in [
            (day(2026, 2, 27), day(2026, 2, 28)),
            (day(2026, 3, 30), day(2026, 3, 31)),
            (day(2026, 4, 29), day(2026, 4, 30)),
        ] {
            travel(eve);
            reconcile_and_pay(&billing, &tenant).await?;
            travel(turn);
            reconcile_and_pay(&billing, &tenant).await?;
        }

        let periods: Vec<(i64, i64)> = invoices(&tenant)
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
        assert_eq!(invoice_amounts(&tenant).await?, vec![500, 500, 500, 500]);

        Ok(())
    });
}

#[test]
fn operators_are_invoiced_but_never_collected_or_churned() {
    run(|| async {
        let billing = billing();
        let operator = admin_pubkey();

        travel(day(2026, 1, 7));
        create_tenant_with(&operator).await?;
        let relay = create_relay(&operator, PLAN_BASIC).await?;
        reconcile(&billing, &operator).await?;

        travel(day(2026, 2, 7));
        reconcile(&billing, &operator).await?;

        // Past the grace period, the operator still owes nothing.
        travel(day(2026, 2, 28));
        reconcile(&billing, &operator).await?;

        assert_eq!(invoice_amounts(&operator).await?, vec![500, 500]);
        for invoice in invoices(&operator).await? {
            assert!(invoice.paid_at.is_some());
            assert_eq!(invoice.method.as_deref(), Some("comp"));
        }
        assert_eq!(get_tenant(&operator).await?.churned_at, None);
        assert_eq!(get_relay(&relay.id).await?.status, RELAY_STATUS_ACTIVE);

        Ok(())
    });
}
