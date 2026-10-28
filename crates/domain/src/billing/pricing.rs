//! Pricing what happened to a tenant's relays.

use super::plans::{self, Plan};
use crate::activity::Activity;
use crate::billing::BillingPeriod;
use crate::billing::InvoiceItem;
use crate::error::{Error, Result};
use crate::tenants::Tenant;

/// A prorated charge with id `id`, or a credit with `sign` -1, for the plan on the
/// activity's snapshot over what remains of its period. `None` for a free plan.
pub fn prorated_item(
    id: String,
    tenant: &Tenant,
    activity: &Activity,
    sign: i64,
    description: &str,
) -> Result<Option<InvoiceItem>> {
    let plan_id = &activity.snapshot.plan_id;
    let plan = Plan::recorded(plan_id)?;
    if !plan.is_paid() {
        return Ok(None);
    }

    let period = tenant.activity_period(activity)?;

    Ok(Some(InvoiceItem {
        id,
        invoice_id: None,
        activity_id: Some(activity.id.clone()),
        tenant_pubkey: activity.tenant_pubkey.clone(),
        relay_id: activity.relay_id.clone(),
        plan_id: plan.id,
        amount: sign * period.prorate(plan.amount, activity.created_at),
        description: description.to_string(),
        created_at: activity.created_at,
        voided_at: None,
    }))
}

impl Plan {
    /// The plan an activity recorded, which billing can't price if the catalog lost it.
    pub fn recorded(id: &str) -> Result<Self> {
        plans::get_plan(id).ok_or_else(|| Error::internal(format!("unknown plan {id}")))
    }
}

impl Tenant {
    /// The period an activity fell in, by the tenant's billing anchor.
    pub fn activity_period(&self, activity: &Activity) -> Result<BillingPeriod> {
        BillingPeriod::at(self.billing_anchor, activity.created_at)
            .ok_or_else(|| Error::internal("billing anchor must be set before pricing activity"))
    }
}
