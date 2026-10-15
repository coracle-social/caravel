//! Billing periods: calendar months counted from a tenant's billing anchor.

use chrono::{DateTime, Months, Utc};

/// One monthly billing period, half-open `[start, end)` so a moment at exactly
/// `end` belongs to the next period.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BillingPeriod {
    pub start: i64,
    pub end: i64,
}

impl BillingPeriod {
    /// The period containing `at` for a tenant anchored at `anchor`, or `None` for a
    /// tenant with no anchor yet, which is one with no billable activity.
    pub fn at(anchor: Option<i64>, at: i64) -> Option<Self> {
        let anchor = DateTime::<Utc>::from_timestamp(anchor?, 0).unwrap_or_default();

        // Walk forward in whole calendar months from the anchor until the next step
        // would pass `at`, so boundaries track months rather than a fixed span.
        let mut start = anchor;
        let mut months = 1u32;
        while let Some(next) = anchor.checked_add_months(Months::new(months)) {
            if next.timestamp() > at {
                break;
            }
            start = next;
            months += 1;
        }

        // Derive `end` from the anchor rather than `start + 1 month`: month-add clamps
        // to the last valid day, so re-adding from a clamped `start` would leave gaps
        // between periods for anchors on the 29th to 31st.
        let end = anchor
            .checked_add_months(Months::new(months))
            .unwrap_or(start);

        Some(Self {
            start: start.timestamp(),
            end: end.timestamp(),
        })
    }

    /// The fraction of this period still unused at `at`, in `[0.0, 1.0]`, with the
    /// remaining time rounded to the nearest day. Rounding by the day bills a relay
    /// created in the first half-day of a period the full plan amount.
    fn fraction_remaining(&self, at: i64) -> f64 {
        const DAY: i64 = 24 * 60 * 60;

        let len = (self.end - self.start) as f64;
        if len <= 0.0 {
            return 1.0;
        }
        let remaining = ((self.end - at) + DAY / 2) / DAY * DAY;
        (remaining as f64 / len).clamp(0.0, 1.0)
    }

    /// Prorate a minor-unit `amount` by the fraction of this period remaining at
    /// `at`, rounded to the nearest unit.
    pub fn prorate(&self, amount: i64, at: i64) -> i64 {
        (amount as f64 * self.fraction_remaining(at)).round() as i64
    }
}
