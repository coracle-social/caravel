//! Payouts: onboarding a tenant to the Stripe Connect account their members pay
//! into, and keeping track of whether it can take payments.

use crate::Core;
use caravel_db::commands;
use caravel_domain::billing::PayoutStatus;
use caravel_util::error::{Error, Result};
use caravel_domain::tenants::Tenant;

/// A Stripe onboarding link for a tenant, creating their Connect account the
/// first time. Stripe sends them back to `return_url`, also when the link goes
/// stale.
pub async fn onboard(core: &Core, tenant: &Tenant, return_url: &str) -> Result<String> {
    let account_id = match &tenant.stripe_account_id {
        Some(account_id) => account_id.clone(),
        None => {
            let account_id = core.stripe.create_connect_account(&tenant.pubkey).await?;
            commands::tenants::set_tenant_stripe_account(
                core.db.pool(),
                &tenant.pubkey,
                &account_id,
                false,
            )
            .await?;
            account_id
        }
    };

    Ok(core
        .stripe
        .create_account_link(&account_id, return_url, return_url)
        .await?)
}

/// Whether a tenant can be paid, read from Stripe and kept on the tenant.
pub async fn refresh_status(core: &Core, tenant: &Tenant) -> Result<PayoutStatus> {
    let fee_percent = core.config.member_fee_percent;
    let Some(account_id) = &tenant.stripe_account_id else {
        return Ok(PayoutStatus {
            account_id: None,
            ready: false,
            fee_percent,
        });
    };

    let ready = core.stripe.is_connect_account_ready(account_id).await?;
    commands::tenants::set_tenant_stripe_account(core.db.pool(), &tenant.pubkey, account_id, ready)
        .await?;

    Ok(PayoutStatus {
        account_id: Some(account_id.clone()),
        ready,
        fee_percent,
    })
}

/// A login link to the tenant's Express dashboard, where their payouts are.
pub async fn dashboard(core: &Core, tenant: &Tenant) -> Result<String> {
    let Some(account_id) = &tenant.stripe_account_id else {
        return Err(Error::conflict(
            "no-payout-account",
            "this tenant hasn't started onboarding",
        ));
    };
    Ok(core.stripe.create_account_login_link(account_id).await?)
}
