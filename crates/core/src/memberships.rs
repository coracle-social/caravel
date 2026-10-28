//! Paid memberships: members paying a relay's tenant, through Stripe Connect, for
//! access to the relay. A membership drives the relay's access list in the
//! transaction that changes it. Paying admits its member, or registers its invite
//! as a claim for whoever joins with it; lapsing removes a member who got in by
//! paying, and tells them why. A sweep keeps every membership in step with Stripe.

use caravel_util::stripe::{MembershipCheckoutParams, Standing, Subscription};

use crate::Core;
use crate::notifications;
use caravel_db::{commands, queries};
use caravel_domain as domain;
use caravel_util::error::{Error, Result};
use caravel_domain::memberships::MemberSource;
use caravel_domain::memberships::{InviteView, Membership, MembershipStatus, Offer};
use caravel_domain::relays::Relay;
use caravel_domain::sessions::SESSION_TTL;
use caravel_util::bus::Change;

/// A checkout not completed in this long never will be: Stripe expires Checkout
/// sessions after a day. One reopened since is waited out.
const PENDING_TTL: i64 = 2 * 24 * 60 * 60;

/// How long past its period's end a membership keeps access while Stripe
/// collects its renewal.
const RENEWAL_GRACE: i64 = 14 * 24 * 60 * 60;

/// What a relay sells memberships for, and whether it can take payment now.
pub async fn get_offer(core: &Core, relay: &Relay) -> Result<Offer> {
    let accepting_payment = queries::tenants::get_tenant(core.db.pool(), &relay.tenant_pubkey)
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
    let membership = queries::memberships::get_membership_by_invite(core.db.pool(), code)
        .await?
        .ok_or(Error::NotFound("invite not found"))?;
    let relay = queries::relays::get_relay(core.db.pool(), &membership.relay_id)
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

// --- Checkout ---

/// The tenant's Connect account, if it can take payments.
async fn payout_account(core: &Core, relay: &Relay) -> Result<String> {
    queries::tenants::get_tenant(core.db.pool(), &relay.tenant_pubkey)
        .await?
        .and_then(|tenant| tenant.payout_account().map(str::to_string))
        .ok_or_else(|| Error::conflict("payouts-not-ready", "this relay can't take payments yet"))
}

/// Open a Stripe Checkout session for `pubkey` to join, or renew, a relay, and
/// return its url. An unpaid or canceled membership is reused, since a relay and
/// pubkey hold one live membership at a time.
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
            let live =
                queries::memberships::get_live_membership(&mut *conn, &relay.id, pubkey).await?;
            if live
                .as_ref()
                .is_some_and(|m| m.status == MembershipStatus::Active)
            {
                return Err(Error::conflict(
                    "already-a-member",
                    "you already have a membership here",
                ));
            }
            // A seat that can't be granted mustn't be sold.
            if !queries::memberships::has_seat(&mut *conn, relay, Some(pubkey)).await? {
                return Err(Error::conflict("member-limit", "this relay is full"));
            }
            if let Some(live) = live {
                return Ok(live);
            }

            let membership = new_membership(&relay.id, Some(pubkey), None, now);
            commands::memberships::insert_membership(conn, &membership).await?;
            Ok(membership)
        })
        .await?;

    let page = domain::memberships::with_return(&relay.portal_url("/"), return_url)?;
    checkout(
        core,
        relay,
        &account,
        &membership,
        &domain::memberships::checkout_success_url(&page),
        &page,
    )
    .await
}

/// Open a Stripe Checkout session buying an invite to a relay, for someone who may
/// have no Nostr account yet, and return its url.
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
            if !queries::memberships::has_seat(&mut *conn, relay, None).await? {
                return Err(Error::conflict("member-limit", "this relay is full"));
            }
            let membership = new_membership(&relay.id, None, Some(&code), now);
            commands::memberships::insert_membership(conn, &membership).await?;
            Ok(membership)
        })
        .await?;

    let success = domain::memberships::with_return(
        &relay.portal_url(&format!("/invites/{code}")),
        return_url,
    )?;
    let cancel = domain::memberships::with_return(&relay.portal_url("/"), return_url)?;
    checkout(
        core,
        relay,
        &account,
        &membership,
        &domain::memberships::checkout_success_url(&success),
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
            commands::memberships::set_customer(core.db.pool(), &membership.id, &customer_id)
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

    let relay = queries::relays::require_relay(core.db.pool(), &membership.relay_id).await?;
    let key = core.relay_key(&relay)?;
    let published = core
        .db
        .with_tx(async |conn| {
            // Joins bind invites under this lock.
            queries::relays::lock_relay(&mut *conn, &relay.id).await?;
            commands::memberships::set_paid(
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
                    commands::notifications::clear(&mut *conn, &relay.id, pubkey).await?;
                    commands::memberships::admit(
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
                    let unused = queries::memberships::get_membership_by_invite(&mut *conn, code)
                        .await?
                        .is_some_and(|invite| invite.pubkey.is_none());
                    if unused {
                        commands::claims::create_claim(conn, &relay.id, code, now).await?;
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
    let membership = queries::memberships::get_live_membership(core.db.pool(), &relay.id, pubkey)
        .await?
        .filter(|m| m.status == MembershipStatus::Active)
        .ok_or(Error::NotFound("membership not found"))?;

    let (_, subscription_id) = membership.subscription()?;
    core.stripe
        .cancel_subscription_at_period_end(subscription_id)
        .await?;
    commands::memberships::set_status(
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

// --- Lapse and release ---

/// End a membership whose paid period ran out: remove a member who got in by
/// paying and tell them how to renew, or revoke an unused invite's claim.
async fn lapse(core: &Core, membership: &Membership, relay: &Relay) -> Result<()> {
    let key = core.relay_key(relay)?;
    let now = core.clock.now();
    let published = core
        .db
        .with_tx(async |conn| {
            commands::memberships::set_status(
                &mut *conn,
                &membership.id,
                MembershipStatus::Lapsed,
                None,
            )
            .await?;
            match (&membership.pubkey, &membership.invite_code) {
                (Some(pubkey), _) => {
                    let paid = queries::memberships::get_member(&mut *conn, &relay.id, pubkey)
                        .await?
                        .is_some_and(|member| member.source == MemberSource::Payment);
                    if paid {
                        commands::memberships::remove(
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
                    commands::claims::revoke_claim(conn, &relay.id, code, now).await?;
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
        let (token, _) = commands::sessions::create_session(core.db.pool(), pubkey, now).await?;
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
            commands::memberships::set_status(
                &mut *conn,
                &membership.id,
                MembershipStatus::Lapsed,
                None,
            )
            .await?;
            if membership.pubkey.is_none()
                && let Some(code) = &membership.invite_code
            {
                commands::claims::revoke_claim(conn, &membership.relay_id, code, now).await?;
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
    for membership in queries::memberships::list_memberships_needing_release(core.db.pool()).await?
    {
        if let Err(error) = release(core, &membership).await {
            tracing::warn!(membership = %membership.id, error = %error, "failed to release a membership");
        }
    }

    for membership in queries::memberships::list_memberships_with_status(
        core.db.pool(),
        MembershipStatus::Pending,
    )
    .await?
    {
        if let Err(error) = settle_pending(core, &membership).await {
            tracing::warn!(membership = %membership.id, error = %error, "failed to settle a checkout");
        }
    }

    for membership in
        queries::memberships::list_memberships_past_period(core.db.pool(), core.clock.now()).await?
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
        commands::memberships::set_status(
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
    let relay = queries::relays::require_relay(core.db.pool(), &membership.relay_id).await?;
    let (customer_id, subscription_id) = membership.subscription()?;
    let now = core.clock.now();

    let subscription = core.stripe.get_subscription(subscription_id).await?;
    let renewed = subscription.current_period_end.filter(|end| *end > now);
    if let Some(period_end) = renewed
        .filter(|_| subscription.standing == Standing::Good && subscription.latest_invoice_paid)
    {
        let status = status_of(&subscription);
        return commands::memberships::set_status(
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
fn status_of(subscription: &Subscription) -> MembershipStatus {
    if subscription.cancel_at_period_end {
        MembershipStatus::Canceled
    } else {
        MembershipStatus::Active
    }
}
