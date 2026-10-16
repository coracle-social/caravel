//! Payouts: the Stripe Connect account a tenant's members pay into, less the
//! platform's fee.

use serde::{Deserialize, Serialize};

use crate::Core;
use crate::error::{Error, Result};
use crate::tenants::{self, Tenant};

/// Whether a tenant can be paid, and what the platform keeps of each payment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PayoutStatus {
    pub account_id: Option<String>,
    pub ready: bool,
    pub fee_percent: f64,
}

/// A Stripe onboarding link for a tenant, creating their Connect account the
/// first time. Stripe sends them back to `return_url`, also when the link goes
/// stale.
pub async fn onboard(core: &Core, tenant: &Tenant, return_url: &str) -> Result<String> {
    let account_id = if tenant.stripe_account_id.is_empty() {
        let account_id = core.stripe.create_connect_account(&tenant.pubkey).await?;
        tenants::set_tenant_stripe_account(core.db.pool(), &tenant.pubkey, &account_id, false)
            .await?;
        account_id
    } else {
        tenant.stripe_account_id.clone()
    };

    Ok(core
        .stripe
        .create_account_link(&account_id, return_url, return_url)
        .await?)
}

/// Whether a tenant can be paid, read from Stripe and kept on the tenant.
pub async fn get_status(core: &Core, tenant: &Tenant) -> Result<PayoutStatus> {
    let fee_percent = core.config.member_fee_percent;
    if tenant.stripe_account_id.is_empty() {
        return Ok(PayoutStatus {
            account_id: None,
            ready: false,
            fee_percent,
        });
    }

    let ready = core
        .stripe
        .is_connect_account_ready(&tenant.stripe_account_id)
        .await?;
    tenants::set_tenant_stripe_account(
        core.db.pool(),
        &tenant.pubkey,
        &tenant.stripe_account_id,
        ready,
    )
    .await?;

    Ok(PayoutStatus {
        account_id: Some(tenant.stripe_account_id.clone()),
        ready,
        fee_percent,
    })
}

/// A login link to the tenant's Express dashboard, where their payouts are.
pub async fn dashboard(core: &Core, tenant: &Tenant) -> Result<String> {
    if tenant.stripe_account_id.is_empty() {
        return Err(Error::conflict(
            "no-payout-account",
            "this tenant hasn't started onboarding",
        ));
    }
    Ok(core
        .stripe
        .create_account_login_link(&tenant.stripe_account_id)
        .await?)
}
