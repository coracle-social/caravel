//! Tenants: the pubkeys that host relays on the platform and pay for them.

use caravel_billing::tenants::Tenant;
use caravel_util::bus::Change;
use caravel_util::error::{Error, Result};

use crate::Core;
use crate::billing;

/// A page of every tenant on the platform, newest first.
pub async fn page_tenants(core: &Core, before: Option<&str>, limit: i64) -> Result<Vec<Tenant>> {
    caravel_billing::tenants::queries::page_tenants(core.db.pool(), before, limit).await
}

/// The tenant for `pubkey`, or a 404.
pub async fn require_tenant(core: &Core, pubkey: &str) -> Result<Tenant> {
    caravel_billing::tenants::queries::require_tenant(core.db.pool(), pubkey).await
}

/// The tenant for `pubkey`, created with a Stripe customer if it doesn't exist.
/// An existing tenant keeps the `return_url` it was created with.
pub async fn ensure_tenant(core: &Core, pubkey: &str, return_url: &str) -> Result<Tenant> {
    let return_url = return_url.trim().trim_end_matches('/');
    if return_url.is_empty() {
        return Err(Error::invalid(
            "return-url-required",
            "return_url is required",
        ));
    }

    if let Some(tenant) =
        caravel_billing::tenants::queries::get_tenant(core.db.pool(), pubkey).await?
    {
        return Ok(tenant);
    }

    let name = core
        .robot
        .fetch_nostr_name(pubkey, core.clock.now())
        .await
        .unwrap_or_else(|| pubkey.chars().take(8).collect());
    let stripe_customer_id = core.stripe.create_customer(pubkey, &name).await?;

    let tenant = Tenant {
        pubkey: pubkey.to_string(),
        return_url: return_url.to_string(),
        created_at: core.clock.now(),
        stripe_customer_id,
        ..Default::default()
    };
    caravel_billing::tenants::commands::insert_tenant(core.db.pool(), &tenant).await?;

    caravel_billing::tenants::queries::get_tenant(core.db.pool(), pubkey)
        .await?
        .ok_or_else(|| Error::internal("tenant missing after insert"))
}

/// Replace the tenant's NWC url, stored encrypted. Empty removes it. Saving one
/// brings a churned tenant back, as new activity does; the next reconcile prices
/// their restored relays.
pub async fn set_tenant_nwc_url(core: &Core, pubkey: &str, nwc_url: &str) -> Result<()> {
    let encrypted = if nwc_url.is_empty() {
        None
    } else {
        Some(core.config.platform_key.encrypt(nwc_url)?)
    };

    caravel_billing::tenants::commands::set_tenant_nwc_url(
        core.db.pool(),
        pubkey,
        encrypted.as_deref(),
    )
    .await?;
    if encrypted.is_some() {
        billing::reactivate_tenant(core, pubkey).await?;
    }
    Ok(())
}

/// Ban a tenant: block relay creation, then forgive what they owe and purge their
/// relays as deleting them does.
pub async fn ban_tenant(core: &Core, pubkey: &str) -> Result<()> {
    let now = core.clock.now();
    let purged = core
        .db
        .with_tx(async |conn| {
            let tenant = caravel_billing::tenants::queries::lock_tenant(conn, pubkey).await?;
            if tenant.banned_at.is_some() {
                return Err(Error::conflict(
                    "tenant-is-banned",
                    "tenant is already banned",
                ));
            }

            caravel_billing::tenants::commands::set_tenant_banned_at(&mut *conn, pubkey, Some(now))
                .await?;
            caravel_billing::tenants::commands::forgive_and_purge_in_tx(conn, pubkey, now).await
        })
        .await?;

    for relay_id in purged {
        core.bus.publish(Change::RelayChanged { relay_id });
    }
    Ok(())
}

/// Delete a tenant without banning them: forgive what they owe and purge their
/// relays. They may sign in and create relays again, under the same subdomains.
pub async fn delete_tenant(core: &Core, pubkey: &str) -> Result<()> {
    let now = core.clock.now();
    let purged = core
        .db
        .with_tx(async |conn| {
            caravel_billing::tenants::queries::lock_tenant(conn, pubkey).await?;
            caravel_billing::tenants::commands::forgive_and_purge_in_tx(conn, pubkey, now).await
        })
        .await?;

    for relay_id in purged {
        core.bus.publish(Change::RelayChanged { relay_id });
    }
    Ok(())
}

/// Lift a tenant's ban. Purged relays stay purged and forgiven charges forgiven.
pub async fn unban_tenant(core: &Core, pubkey: &str) -> Result<()> {
    let tenant = caravel_billing::tenants::queries::require_tenant(core.db.pool(), pubkey).await?;
    if tenant.banned_at.is_none() {
        return Err(Error::conflict("tenant-not-banned", "tenant is not banned"));
    }

    caravel_billing::tenants::commands::set_tenant_banned_at(core.db.pool(), pubkey, None).await
}
