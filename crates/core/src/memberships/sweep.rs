//! Keeping memberships in step with Stripe: renewing, lapsing and releasing
//! the members they admitted.

use caravel_billing::relays::{Membership, MembershipStatus, Relay};
use caravel_relays::access::MemberSource;
use caravel_util::bus::Change;
use caravel_util::error::{Error, Result};
use caravel_util::stripe::{Standing, Subscription};

use crate::Core;
use crate::notifications;
use caravel_operations::sessions::SESSION_TTL;

use super::checkout::{PENDING_TTL, activate};

/// How long past its period's end a membership keeps access while Stripe
/// collects its renewal.
const RENEWAL_GRACE: i64 = 14 * 24 * 60 * 60;

// --- Lapse and release ---

/// End a membership whose paid period ran out: remove a member who got in by
/// paying and tell them how to renew, or revoke an unused invite's claim.
async fn lapse(core: &Core, membership: &Membership, relay: &Relay) -> Result<()> {
    let key = core.relay_key(relay)?;
    let now = core.clock.now();
    let published = core
        .db
        .with_tx(async |conn| {
            caravel_billing::relays::commands::membership::set_status(
                &mut *conn,
                &membership.id,
                MembershipStatus::Lapsed,
                None,
            )
            .await?;
            match (&membership.pubkey, &membership.invite_code) {
                (Some(pubkey), _) => {
                    let paid =
                        caravel_relays::access::queries::get_member(&mut *conn, &relay.id, pubkey)
                            .await?
                            .is_some_and(|member| member.source == MemberSource::Payment);
                    if paid {
                        caravel_relays::access::commands::remove(
                            conn,
                            &key,
                            &relay.id,
                            &relay.tenant_pubkey,
                            pubkey,
                            now,
                        )
                        .await
                    } else {
                        Ok(Vec::new())
                    }
                }
                (None, Some(code)) => {
                    caravel_relays::access::commands::revoke_claim(conn, &relay.id, code, now)
                        .await?;
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

    if let Some(pubkey) = &membership.pubkey {
        let (token, _) =
            caravel_operations::sessions::commands::create_session(core.db.pool(), pubkey, now)
                .await?;
        let mut renew = url::Url::parse(&relay.portal_url("/")).map_err(Error::internal)?;
        renew.query_pairs_mut().append_pair("token", &token);
        let message = format!(
            "Your membership of {} has ended. Renew at {renew}",
            relay.display_name()
        );
        notifications::notify(core, &relay.id, pubkey, &message, Some(now + SESSION_TTL)).await?;
    }

    Ok(())
}

/// End a membership whose relay stopped selling them. Nothing more is charged and
/// nothing is refunded; a member stays on the relay, and an unused invite stops
/// working.
async fn release(core: &Core, membership: &Membership) -> Result<()> {
    if let Some(customer_id) = &membership.stripe_customer_id {
        for session_id in core.stripe.list_open_checkout_sessions(customer_id).await? {
            core.stripe.expire_checkout_session(&session_id).await?;
        }
        for subscription_id in core.stripe.list_live_subscriptions(customer_id).await? {
            core.stripe.cancel_subscription(&subscription_id).await?;
        }
    }

    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            caravel_billing::relays::commands::membership::set_status(
                &mut *conn,
                &membership.id,
                MembershipStatus::Lapsed,
                None,
            )
            .await?;
            if membership.pubkey.is_none()
                && let Some(code) = &membership.invite_code
            {
                caravel_relays::access::commands::revoke_claim(
                    conn,
                    &membership.relay_id,
                    code,
                    now,
                )
                .await?;
            }
            Ok::<_, Error>(())
        })
        .await
}

// --- Sweep ---

/// Bring every membership in step with Stripe: release those whose relay stopped
/// selling them, settle checkouts nobody came back from, and settle those whose
/// paid period has run out. A paying membership is read from Stripe once per
/// period, after it ends, so no webhook is needed and none can be missed.
pub async fn sweep(core: &Core) -> Result<()> {
    for membership in
        caravel_billing::relays::queries::membership::list_memberships_needing_release(
            core.db.pool(),
        )
        .await?
    {
        if let Err(error) = release(core, &membership).await {
            tracing::warn!(membership = %membership.id, error = %error, "failed to release a membership");
        }
    }

    for membership in caravel_billing::relays::queries::membership::list_memberships_with_status(
        core.db.pool(),
        MembershipStatus::Pending,
    )
    .await?
    {
        if let Err(error) = settle_pending(core, &membership).await {
            tracing::warn!(membership = %membership.id, error = %error, "failed to settle a checkout");
        }
    }

    for membership in caravel_billing::relays::queries::membership::list_memberships_past_period(
        core.db.pool(),
        core.clock.now(),
    )
    .await?
    {
        if let Err(error) = renew_or_lapse(core, &membership).await {
            tracing::warn!(membership = %membership.id, error = %error, "failed to settle a membership");
        }
    }

    Ok(())
}

/// Activate a membership whose checkout completed without its payer coming back,
/// or give up on one that never will.
async fn settle_pending(core: &Core, membership: &Membership) -> Result<()> {
    let expired = membership.created_at < core.clock.now() - PENDING_TTL;
    if let Some(customer_id) = &membership.stripe_customer_id {
        // Listed before the subscriptions, so a checkout completed in between shows as paid.
        let checking_out = expired
            && !core
                .stripe
                .list_open_checkout_sessions(customer_id)
                .await?
                .is_empty();
        if let Some(subscription_id) = core
            .stripe
            .list_live_subscriptions(customer_id)
            .await?
            .first()
            && activate(core, membership, customer_id, subscription_id).await?
        {
            return Ok(());
        }
        if checking_out {
            return Ok(());
        }
    }

    if expired {
        caravel_billing::relays::commands::membership::set_status(
            core.db.pool(),
            &membership.id,
            MembershipStatus::Lapsed,
            None,
        )
        .await?;
    }
    Ok(())
}

/// Ask Stripe where a membership whose period ended stands: a renewal paid for
/// carries on, one Stripe is still collecting is waited for until RENEWAL_GRACE
/// runs out, a renewal paid in a newer checkout takes over, and anything else
/// lapses it.
async fn renew_or_lapse(core: &Core, membership: &Membership) -> Result<()> {
    let relay =
        caravel_billing::relays::queries::require_relay(core.db.pool(), &membership.relay_id)
            .await?;
    let (customer_id, subscription_id) = membership.subscription()?;
    let now = core.clock.now();

    let subscription = core.stripe.get_subscription(subscription_id).await?;
    let renewed = subscription.current_period_end.filter(|end| *end > now);
    if let Some(period_end) = renewed
        .filter(|_| subscription.standing == Standing::Good && subscription.latest_invoice_paid)
    {
        let status = status_of(&subscription);
        return caravel_billing::relays::commands::membership::set_status(
            core.db.pool(),
            &membership.id,
            status,
            Some(period_end),
        )
        .await;
    }

    // Not cycled yet, drafting the renewal's invoice, or retrying a failed payment.
    let collecting = subscription.standing != Standing::Stopped
        && (renewed.is_some() || !subscription.cancel_at_period_end);
    let in_grace = membership
        .current_period_end
        .is_some_and(|end| now < end + RENEWAL_GRACE);
    if collecting && in_grace {
        return Ok(());
    }

    let live = core.stripe.list_live_subscriptions(customer_id).await?;
    // Nothing may charge for this subscription once its membership moves on.
    if live.iter().any(|id| id == subscription_id) {
        core.stripe.cancel_subscription(subscription_id).await?;
    }
    if let Some(renewal) = live.iter().find(|id| *id != subscription_id)
        && activate(core, membership, customer_id, renewal).await?
    {
        return Ok(());
    }

    lapse(core, membership, &relay).await
}

/// The status of a membership paying through a subscription with a period still
/// running.
pub(super) fn status_of(subscription: &Subscription) -> MembershipStatus {
    if subscription.cancel_at_period_end {
        MembershipStatus::Canceled
    } else {
        MembershipStatus::Active
    }
}
