//! Opening a membership checkout, and the changes its payment makes.

use caravel_billing::relays::{Membership, MembershipStatus, Relay};
use caravel_relays::access::MemberSource;
use caravel_util::bus::Change;
use caravel_util::error::{Error, Result};
use caravel_util::stripe::{MembershipCheckoutParams, Standing};

use crate::Core;

use super::sweep::status_of;

/// A checkout not completed in this long never will be: Stripe expires Checkout
/// sessions after a day. One reopened since is waited out.
pub(super) const PENDING_TTL: i64 = 2 * 24 * 60 * 60;

// --- Checkout ---

/// The tenant's Connect account, if it can take payments.
async fn payout_account(core: &Core, relay: &Relay) -> Result<String> {
    caravel_billing::tenants::queries::get_tenant(core.db.pool(), &relay.tenant_pubkey)
        .await?
        .and_then(|tenant| tenant.payout_account().map(str::to_string))
        .ok_or_else(|| Error::conflict("payouts-not-ready", "this relay can't take payments yet"))
}

/// Open a Stripe Checkout session for `pubkey` to join, or renew, a relay, and
/// return its url. An unpaid or canceled membership is reused, since a relay and
/// pubkey hold one live membership at a time. A pubkey already admitted another
/// way, without a membership, is refused: it has no seat to buy.
pub async fn open_checkout(
    core: &Core,
    relay: &Relay,
    pubkey: &str,
    return_url: Option<&str>,
) -> Result<String> {
    let account = payout_account(core, relay).await?;
    let now = core.clock.now();

    let membership = core
        .db
        .with_tx(async |conn| {
            let live = caravel_billing::relays::queries::membership::get_live_membership(
                &mut *conn, &relay.id, pubkey,
            )
            .await?;
            if live
                .as_ref()
                .is_some_and(|m| m.status == MembershipStatus::Active)
            {
                return Err(Error::conflict(
                    "already-a-member",
                    "you already have a membership here",
                ));
            }
            // A comped, claimed or publicly joined member already has access.
            if !live
                .as_ref()
                .is_some_and(|m| m.status == MembershipStatus::Canceled)
                && caravel_relays::access::queries::get_member(&mut *conn, &relay.id, pubkey)
                    .await?
                    .is_some()
            {
                return Err(Error::conflict(
                    "already-a-member",
                    "you are already a member here",
                ));
            }
            // A seat that can't be granted mustn't be sold.
            if !caravel_billing::relays::queries::membership::has_seat(
                &mut *conn,
                relay,
                Some(pubkey),
            )
            .await?
            {
                return Err(Error::conflict("member-limit", "this relay is full"));
            }
            if let Some(live) = live {
                return Ok(live);
            }

            let membership = new_membership(&relay.id, Some(pubkey), None, now);
            caravel_billing::relays::commands::membership::insert_membership(conn, &membership)
                .await?;
            Ok(membership)
        })
        .await?;

    let page = caravel_billing::relays::with_return(&relay.portal_url("/"), return_url)?;
    checkout(
        core,
        relay,
        &account,
        &membership,
        &caravel_billing::relays::checkout_success_url(&page),
        &page,
    )
    .await
}

/// Open a Stripe Checkout session buying an invite to a relay, for someone who may
/// have no Nostr account yet, and return its url. An unpaid invite holds its seat
/// until it lapses, so repeat visits can't mint checkouts without bound.
pub async fn open_invite_checkout(
    core: &Core,
    relay: &Relay,
    return_url: Option<&str>,
) -> Result<String> {
    let account = payout_account(core, relay).await?;
    let now = core.clock.now();
    let code = uuid::Uuid::new_v4().simple().to_string();

    let membership = core
        .db
        .with_tx(async |conn| {
            if !caravel_billing::relays::queries::membership::has_invite_seat(&mut *conn, relay)
                .await?
            {
                return Err(Error::conflict("member-limit", "this relay is full"));
            }
            let membership = new_membership(&relay.id, None, Some(&code), now);
            caravel_billing::relays::commands::membership::insert_membership(conn, &membership)
                .await?;
            Ok(membership)
        })
        .await?;

    let success = caravel_billing::relays::with_return(
        &relay.portal_url(&format!("/invites/{code}")),
        return_url,
    )?;
    let cancel = caravel_billing::relays::with_return(&relay.portal_url("/"), return_url)?;
    checkout(
        core,
        relay,
        &account,
        &membership,
        &caravel_billing::relays::checkout_success_url(&success),
        &cancel,
    )
    .await
}

fn new_membership(
    relay_id: &str,
    pubkey: Option<&str>,
    invite_code: Option<&str>,
    now: i64,
) -> Membership {
    Membership {
        id: uuid::Uuid::new_v4().to_string(),
        relay_id: relay_id.to_string(),
        pubkey: pubkey.map(str::to_string),
        invite_code: invite_code.map(str::to_string),
        stripe_customer_id: None,
        stripe_subscription_id: None,
        status: MembershipStatus::Pending,
        current_period_end: None,
        bound_at: None,
        created_at: now,
    }
}

async fn checkout(
    core: &Core,
    relay: &Relay,
    account: &str,
    membership: &Membership,
    success_url: &str,
    cancel_url: &str,
) -> Result<String> {
    let customer_id = match &membership.stripe_customer_id {
        Some(customer_id) => customer_id.clone(),
        None => {
            let customer_id = core
                .stripe
                .create_member_customer(&membership.id, &relay.id)
                .await?;
            caravel_billing::relays::commands::membership::set_customer(
                core.db.pool(),
                &membership.id,
                &customer_id,
            )
            .await?;
            customer_id
        }
    };

    // Renewing a canceled membership charges from the end of the period it paid for.
    let now = core.clock.now();
    let billing_cycle_anchor = Some(membership)
        .filter(|m| m.status == MembershipStatus::Canceled)
        .and_then(|m| m.current_period_end)
        .filter(|end| *end > now + 60 * 60);

    Ok(core
        .stripe
        .create_membership_checkout(&MembershipCheckoutParams {
            customer_id: &customer_id,
            destination_account_id: account,
            relay_id: &relay.id,
            membership_id: &membership.id,
            product_name: &format!("Membership of {}", relay.display_name()),
            amount: relay.member_price,
            interval: relay.member_interval.as_str(),
            application_fee_percent: core.config.member_fee_percent,
            billing_cycle_anchor,
            success_url,
            cancel_url,
        })
        .await?)
}

/// Settle a checkout its payer came back from, so they are let in while still on
/// the page. Returns whether the membership is paid for.
pub async fn reconcile_checkout(
    core: &Core,
    membership: &Membership,
    session_id: &str,
) -> Result<bool> {
    let checkout = core.stripe.get_membership_checkout(session_id).await?;
    if checkout.membership_id.as_deref() != Some(membership.id.as_str()) {
        return Err(Error::conflict(
            "session-mismatch",
            "this checkout session is for a different membership",
        ));
    }

    if membership.status.is_paid() {
        return Ok(true);
    }

    match (checkout.complete, checkout.subscription_id) {
        (true, Some(subscription_id)) => {
            let customer_id = checkout
                .customer_id
                .or_else(|| membership.stripe_customer_id.clone())
                .ok_or_else(|| Error::internal("a paid checkout has no customer"))?;
            activate(core, membership, &customer_id, &subscription_id).await
        }
        _ => Ok(false),
    }
}

/// Record that a membership pays through `subscription_id` and grant it access:
/// admit its member, or register its invite's claim while nobody has used it.
/// Returns whether the subscription is paying for a period still running.
pub async fn activate(
    core: &Core,
    membership: &Membership,
    customer_id: &str,
    subscription_id: &str,
) -> Result<bool> {
    let subscription = core.stripe.get_subscription(subscription_id).await?;
    let now = core.clock.now();
    let Some(period_end) = subscription
        .current_period_end
        .filter(|end| subscription.standing == Standing::Good && *end > now)
    else {
        return Ok(false);
    };
    let status = status_of(&subscription);

    let relay =
        caravel_billing::relays::queries::require_relay(core.db.pool(), &membership.relay_id)
            .await?;
    let key = core.relay_key(&relay)?;
    let published = core
        .db
        .with_tx(async |conn| {
            // Joins bind invites under this lock.
            caravel_billing::relays::queries::lock_relay(&mut *conn, &relay.id).await?;
            caravel_billing::relays::commands::membership::set_paid(
                &mut *conn,
                &membership.id,
                status,
                period_end,
                customer_id,
                subscription_id,
            )
            .await?;

            match (&membership.pubkey, &membership.invite_code) {
                (Some(pubkey), _) => {
                    caravel_relays::notify::commands::clear(&mut *conn, &relay.id, pubkey).await?;
                    caravel_relays::access::commands::admit(
                        conn,
                        &key,
                        &relay.id,
                        pubkey,
                        MemberSource::Payment,
                        now,
                    )
                    .await
                }
                (None, Some(code)) => {
                    // The caller's copy may predate the join that spent the invite.
                    let unused =
                        caravel_billing::relays::queries::membership::get_membership_by_invite(
                            &mut *conn, code,
                        )
                        .await?
                        .is_some_and(|invite| invite.pubkey.is_none());
                    if unused {
                        caravel_relays::access::commands::create_claim(conn, &relay.id, code, now)
                            .await?;
                    }
                    Ok(Vec::new())
                }
                (None, None) => Ok(Vec::new()),
            }
        })
        .await?;

    core.bus.publish(Change::AccessChanged {
        relay_id: relay.id.clone(),
        published,
    });
    Ok(true)
}

/// Stop a member's membership of a relay from renewing. It keeps its access until
/// its paid period ends.
pub async fn cancel_renewal(core: &Core, relay: &Relay, pubkey: &str) -> Result<Membership> {
    let membership = caravel_billing::relays::queries::membership::get_live_membership(
        core.db.pool(),
        &relay.id,
        pubkey,
    )
    .await?
    .filter(|m| m.status == MembershipStatus::Active)
    .ok_or(Error::NotFound("membership not found"))?;

    let (_, subscription_id) = membership.subscription()?;
    core.stripe
        .cancel_subscription_at_period_end(subscription_id)
        .await?;
    caravel_billing::relays::commands::membership::set_status(
        core.db.pool(),
        &membership.id,
        MembershipStatus::Canceled,
        None,
    )
    .await?;

    Ok(Membership {
        status: MembershipStatus::Canceled,
        ..membership
    })
}
