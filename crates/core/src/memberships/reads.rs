//! Reading what a relay sells and who holds a membership of it.

use caravel_billing::relays::{InviteView, Membership, Offer, Relay};
use caravel_util::error::{Error, Result};

use crate::Core;

// --- Reads ---

/// The caller's live membership of a relay, if they hold one.
pub async fn live_membership(
    core: &Core,
    relay_id: &str,
    pubkey: &str,
) -> Result<Option<Membership>> {
    caravel_billing::relays::queries::membership::get_live_membership(
        core.db.pool(),
        relay_id,
        pubkey,
    )
    .await
}

/// Every live membership a pubkey holds, newest first.
pub async fn memberships_for_pubkey(core: &Core, pubkey: &str) -> Result<Vec<Membership>> {
    caravel_billing::relays::queries::membership::list_live_memberships_for_pubkey(
        core.db.pool(),
        pubkey,
    )
    .await
}

/// A relay's memberships, newest first.
pub async fn memberships_for_relay(core: &Core, relay_id: &str) -> Result<Vec<Membership>> {
    caravel_billing::relays::queries::membership::list_memberships_for_relay(
        core.db.pool(),
        relay_id,
    )
    .await
}

/// The membership an invite code belongs to, if any.
pub async fn membership_by_invite(core: &Core, code: &str) -> Result<Option<Membership>> {
    caravel_billing::relays::queries::membership::get_membership_by_invite(core.db.pool(), code)
        .await
}

/// What a relay sells memberships for, and whether it can take payment now.
pub async fn get_offer(core: &Core, relay: &Relay) -> Result<Offer> {
    let accepting_payment =
        caravel_billing::tenants::queries::get_tenant(core.db.pool(), &relay.tenant_pubkey)
            .await?
            .is_some_and(|tenant| tenant.payout_account().is_some());

    Ok(Offer {
        relay: relay.profile(),
        price: relay.member_price,
        interval: relay.member_interval,
        accepting_payment: accepting_payment && relay.charges_members(),
    })
}

/// An invite as its buyer sees it.
pub async fn get_invite(core: &Core, code: &str) -> Result<InviteView> {
    let membership = caravel_billing::relays::queries::membership::get_membership_by_invite(
        core.db.pool(),
        code,
    )
    .await?
    .ok_or(Error::NotFound("invite not found"))?;
    let relay = caravel_billing::relays::queries::get_relay(core.db.pool(), &membership.relay_id)
        .await?
        .ok_or(Error::NotFound("invite not found"))?;
    let offer = get_offer(core, &relay).await?;

    let bound = membership.pubkey.is_some();
    let invite_url = if membership.status.is_paid() && !bound {
        Some(relay.invite_url(&core.config, code)?)
    } else {
        None
    };

    Ok(InviteView {
        offer,
        status: membership.status,
        bound,
        invite_url,
    })
}
