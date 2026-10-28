//! Paid memberships, ported from merchant's member tests and grown to cover what
//! moved into the relay: checkouts and invites against a fake Stripe, joins that
//! bind paid invites, and the sweep that renews, lapses and releases memberships.

mod common;

use anyhow::Result;
use caravel_billing::relays::{Membership, MembershipStatus, PLAN_BASIC, PLAN_FREE};
use caravel_billing::relays::{Relay, RelaySettings};
use caravel_core::testing::sign_event;
use caravel_core::{Core, hosting, management, memberships, sessions};
use caravel_relays::access::MemberSource;
use caravel_relays::policy::Join;
use caravel_util::error::Error;
use caravel_util::nostr::kinds;
use common::stripe::FakeStripe;
use common::*;
use coracle_lib::keys::SecretKey;

const DAY: i64 = 24 * 60 * 60;

/// The sweep reads every membership on the shared database, so the tests take turns.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Fixture {
    core: Core,
    stripe: FakeStripe,
    relay: Relay,
    _turn: tokio::sync::MutexGuard<'static, ()>,
}

/// A relay on the free plan (ten members) selling memberships at $5 a month, whose
/// tenant can take payments.
async fn fixture() -> Result<Fixture> {
    let turn = SERIAL.lock().await;
    let stripe = FakeStripe::start().await;
    let mut core = core().await;
    core.stripe = stripe.stripe();
    travel(&core, day(2030, 3, 1));

    let tenant = create_tenant(&core).await?;
    caravel_billing::tenants::commands::set_tenant_stripe_account(
        core.db.pool(),
        &tenant,
        "acct_ready",
        true,
    )
    .await?;
    let relay = create_relay(&core, &tenant, "free").await?;
    let relay = price(&core, relay, 500).await?;
    Ok(Fixture {
        core,
        stripe,
        relay,
        _turn: turn,
    })
}

async fn price(core: &Core, relay: Relay, member_price: i64) -> Result<Relay> {
    let settings = RelaySettings {
        member_price: Some(member_price),
        ..Default::default()
    };
    Ok(hosting::update_relay(core, relay, settings).await?)
}

fn pubkey() -> String {
    SecretKey::generate().public_key().to_hex()
}

async fn live(f: &Fixture, pubkey: &str) -> Result<Option<Membership>> {
    Ok(
        caravel_billing::relays::queries::membership::get_live_membership(
            f.core.db.pool(),
            &f.relay.id,
            pubkey,
        )
        .await?,
    )
}

async fn member_source(f: &Fixture, pubkey: &str) -> Result<Option<MemberSource>> {
    Ok(
        caravel_relays::access::queries::get_member(f.core.db.pool(), &f.relay.id, pubkey)
            .await?
            .map(|member| member.source),
    )
}

/// Pay for a membership through checkout, coming back to settle it.
async fn pay(f: &Fixture, pubkey: &str) -> Result<Membership> {
    memberships::open_checkout(&f.core, &f.relay, pubkey, None).await?;
    let session = f.stripe.last_session();
    f.stripe
        .complete(&session.id, f.core.clock.now() + 30 * DAY);
    let membership = live(f, pubkey).await?.expect("no membership");
    assert!(memberships::reconcile_checkout(&f.core, &membership, &session.id).await?);
    Ok(live(f, pubkey).await?.expect("no membership"))
}

/// Buy an invite and settle its checkout, returning its code.
async fn buy_invite(f: &Fixture) -> Result<String> {
    memberships::open_invite_checkout(&f.core, &f.relay, None).await?;
    let session = f.stripe.last_session();
    f.stripe
        .complete(&session.id, f.core.clock.now() + 30 * DAY);
    let membership = caravel_billing::relays::queries::membership::list_memberships_for_relay(
        f.core.db.pool(),
        &f.relay.id,
    )
    .await?
    .into_iter()
    .find(|membership| membership.id == session.membership_id)
    .expect("no invite");
    assert!(memberships::reconcile_checkout(&f.core, &membership, &session.id).await?);
    Ok(membership.invite_code.expect("no invite code"))
}

/// Join the relay presenting `code`, as the relay does for a NIP-43 request.
async fn join(core: &Core, relay: &Relay, joiner: &SecretKey, code: &str) -> Result<(), Error> {
    let key = core.relay_key(relay)?;
    let event = sign_event(
        joiner,
        kinds::RELAY_JOIN,
        "",
        &[&["-"], &["claim", code]],
        core.clock.now(),
    );
    let join = Join::Claim(code.to_string());
    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            caravel_billing::relays::commands::membership::admit_join(
                conn, &key, relay, &event, &join, now,
            )
            .await
        })
        .await
        .map(drop)
}

#[tokio::test]
async fn a_paying_member_is_admitted() -> Result<()> {
    let f = fixture().await?;
    let member = pubkey();

    let membership = pay(&f, &member).await?;
    assert_eq!(membership.status, MembershipStatus::Active);
    assert_eq!(
        membership.current_period_end,
        Some(f.core.clock.now() + 30 * DAY)
    );
    assert_eq!(
        member_source(&f, &member).await?,
        Some(MemberSource::Payment)
    );

    // The charge goes to the tenant's account, less the platform's cut.
    let session = f.stripe.last_session();
    assert_eq!(
        session.form["subscription_data[transfer_data][destination]"],
        "acct_ready"
    );
    assert_eq!(
        session.form["subscription_data[application_fee_percent]"],
        "10"
    );
    assert_eq!(
        session.form["line_items[0][price_data][unit_amount]"],
        "500"
    );
    assert_eq!(
        session.form["line_items[0][price_data][recurring][interval]"],
        "month"
    );
    assert_eq!(
        session.form["success_url"],
        format!(
            "https://{}/?checkout={{CHECKOUT_SESSION_ID}}",
            f.relay.host()
        )
    );
    Ok(())
}

#[tokio::test]
async fn a_checkout_is_refused_when_it_cant_be_granted() -> Result<()> {
    let f = fixture().await?;
    let member = pubkey();
    pay(&f, &member).await?;

    let again = memberships::open_checkout(&f.core, &f.relay, &member, None).await;
    assert!(matches!(
        again,
        Err(Error::Conflict {
            code: "already-a-member",
            ..
        })
    ));

    // Ten members fill the free plan, the owner among them.
    for _ in 0..8 {
        management::add_member(&f.core, &f.relay, &pubkey()).await?;
    }
    let full = memberships::open_checkout(&f.core, &f.relay, &pubkey(), None).await;
    assert!(matches!(
        full,
        Err(Error::Conflict {
            code: "member-limit",
            ..
        })
    ));

    caravel_billing::tenants::commands::set_tenant_stripe_account(
        f.core.db.pool(),
        &f.relay.tenant_pubkey,
        "acct_ready",
        false,
    )
    .await?;
    let unpaid = memberships::open_checkout(&f.core, &f.relay, &pubkey(), None).await;
    assert!(matches!(
        unpaid,
        Err(Error::Conflict {
            code: "payouts-not-ready",
            ..
        })
    ));
    Ok(())
}

#[tokio::test]
async fn a_session_settles_only_its_own_membership() -> Result<()> {
    let f = fixture().await?;
    let (first, second) = (pubkey(), pubkey());
    memberships::open_checkout(&f.core, &f.relay, &first, None).await?;
    let first_session = f.stripe.last_session();
    memberships::open_checkout(&f.core, &f.relay, &second, None).await?;

    let second_membership = live(&f, &second).await?.unwrap();
    let mismatch =
        memberships::reconcile_checkout(&f.core, &second_membership, &first_session.id).await;
    assert!(matches!(
        mismatch,
        Err(Error::Conflict {
            code: "session-mismatch",
            ..
        })
    ));

    // An unpaid session settles nothing.
    let first_membership = live(&f, &first).await?.unwrap();
    assert!(!memberships::reconcile_checkout(&f.core, &first_membership, &first_session.id).await?);
    Ok(())
}

// Ported from merchant: an invite's link shows only while it is paid for and unused.
#[tokio::test]
async fn an_invite_goes_to_whoever_joins_with_it_first() -> Result<()> {
    let f = fixture().await?;
    let code = buy_invite(&f).await?;

    let invite = memberships::get_invite(&f.core, &code).await?;
    assert!(!invite.bound);
    assert_eq!(
        invite.invite_url.as_deref(),
        Some(
            format!(
                "https://app.flotilla.test/join?r={}&c={code}",
                f.relay.host()
            )
            .as_str()
        )
    );

    let joiner = SecretKey::generate();
    join(&f.core, &f.relay, &joiner, &code).await?;
    let joiner_pubkey = joiner.public_key().to_hex();
    assert_eq!(
        member_source(&f, &joiner_pubkey).await?,
        Some(MemberSource::Payment)
    );
    let bound = live(&f, &joiner_pubkey)
        .await?
        .expect("the invite wasn't bound");
    assert_eq!(bound.invite_code.as_deref(), Some(code.as_str()));
    assert_eq!(bound.bound_at, Some(f.core.clock.now()));

    let invite = memberships::get_invite(&f.core, &code).await?;
    assert!(invite.bound);
    assert_eq!(invite.invite_url, None);

    // The code is spent.
    let late = join(&f.core, &f.relay, &SecretKey::generate(), &code).await;
    assert!(matches!(
        late,
        Err(Error::Conflict {
            code: "invalid-claim",
            ..
        })
    ));
    Ok(())
}

// The sweep may settle an invite from a copy it read before the invite was used.
#[tokio::test]
async fn settling_an_invite_from_a_stale_copy_leaves_it_spent() -> Result<()> {
    let f = fixture().await?;
    memberships::open_invite_checkout(&f.core, &f.relay, None).await?;
    let session = f.stripe.last_session();
    let subscription_id = f
        .stripe
        .complete(&session.id, f.core.clock.now() + 30 * DAY);
    let stale = caravel_billing::relays::queries::membership::list_memberships_for_relay(
        f.core.db.pool(),
        &f.relay.id,
    )
    .await?
    .into_iter()
    .find(|membership| membership.id == session.membership_id)
    .expect("no invite");
    let code = stale.invite_code.clone().expect("no invite code");
    assert!(memberships::reconcile_checkout(&f.core, &stale, &session.id).await?);
    join(&f.core, &f.relay, &SecretKey::generate(), &code).await?;

    assert!(memberships::activate(&f.core, &stale, &session.customer, &subscription_id).await?);
    assert!(
        !caravel_relays::access::queries::is_live_claim(f.core.db.pool(), &f.relay.id, &code)
            .await?
    );
    Ok(())
}

// Two joins racing on one paid invite: exactly one gets it.
#[tokio::test]
async fn racing_joins_bind_a_paid_invite_once() -> Result<()> {
    let f = fixture().await?;
    let code = buy_invite(&f).await?;
    let (first, second) = (SecretKey::generate(), SecretKey::generate());

    let (a, b) = tokio::join!(
        join(&f.core, &f.relay, &first, &code),
        join(&f.core, &f.relay, &second, &code)
    );
    assert_eq!([a.is_ok(), b.is_ok()].iter().filter(|ok| **ok).count(), 1);

    let bound: Vec<Membership> =
        caravel_billing::relays::queries::membership::list_memberships_for_relay(
            f.core.db.pool(),
            &f.relay.id,
        )
        .await?
        .into_iter()
        .filter(|membership| membership.pubkey.is_some())
        .collect();
    assert_eq!(bound.len(), 1);
    let winner = bound[0].pubkey.clone().unwrap();
    assert_eq!(
        member_source(&f, &winner).await?,
        Some(MemberSource::Payment)
    );
    Ok(())
}

// Ported from merchant: an unused paid invite holds a seat until it lapses.
#[tokio::test]
async fn an_unused_paid_invite_holds_a_seat() -> Result<()> {
    let f = fixture().await?;
    let code = buy_invite(&f).await?;
    assert_eq!(
        caravel_billing::relays::queries::membership::count_reserved_seats(
            f.core.db.pool(),
            &f.relay.id
        )
        .await?,
        1
    );

    join(&f.core, &f.relay, &SecretKey::generate(), &code).await?;
    assert_eq!(
        caravel_billing::relays::queries::membership::count_reserved_seats(
            f.core.db.pool(),
            &f.relay.id
        )
        .await?,
        0
    );
    Ok(())
}

#[tokio::test]
async fn a_renewed_membership_carries_on() -> Result<()> {
    let f = fixture().await?;
    let member = pubkey();
    let membership = pay(&f, &member).await?;

    let period_end = membership.current_period_end.unwrap();
    f.stripe.set_period_end(
        membership.stripe_subscription_id.as_deref().unwrap(),
        period_end + 30 * DAY,
    );
    travel(&f.core, period_end + 1);
    memberships::sweep(&f.core).await?;

    let renewed = live(&f, &member).await?.unwrap();
    assert_eq!(renewed.status, MembershipStatus::Active);
    assert_eq!(renewed.current_period_end, Some(period_end + 30 * DAY));
    assert!(member_source(&f, &member).await?.is_some());
    Ok(())
}

// Stripe moves a subscription to its next period before it collects for it.
#[tokio::test]
async fn a_renewal_counts_once_stripe_collects_it() -> Result<()> {
    let f = fixture().await?;
    let member = pubkey();
    let membership = pay(&f, &member).await?;
    let subscription_id = membership.stripe_subscription_id.clone().unwrap();
    let period_end = membership.current_period_end.unwrap();

    // Not cycled yet, then drafting the renewal's invoice, then retrying its charge.
    travel(&f.core, period_end + 1);
    memberships::sweep(&f.core).await?;
    f.stripe
        .start_renewal(&subscription_id, period_end + 30 * DAY);
    memberships::sweep(&f.core).await?;
    f.stripe
        .set_past_due(&subscription_id, period_end + 30 * DAY);
    travel(&f.core, period_end + 3 * DAY);
    memberships::sweep(&f.core).await?;
    let waiting = live(&f, &member).await?.unwrap();
    assert_eq!(waiting.status, MembershipStatus::Active);
    assert_eq!(waiting.current_period_end, Some(period_end));
    assert!(member_source(&f, &member).await?.is_some());

    f.stripe.pay_renewal(&subscription_id);
    memberships::sweep(&f.core).await?;
    assert_eq!(
        live(&f, &member).await?.unwrap().current_period_end,
        Some(period_end + 30 * DAY)
    );
    Ok(())
}

// While Stripe retries a renewal, the member keeps access for RENEWAL_GRACE.
#[tokio::test]
async fn a_renewal_stripe_cant_collect_lapses_after_its_grace() -> Result<()> {
    let f = fixture().await?;
    let member = pubkey();
    let membership = pay(&f, &member).await?;
    let subscription_id = membership.stripe_subscription_id.clone().unwrap();
    let period_end = membership.current_period_end.unwrap();

    f.stripe
        .set_past_due(&subscription_id, period_end + 30 * DAY);
    travel(&f.core, period_end + 14 * DAY - 60);
    memberships::sweep(&f.core).await?;
    assert!(live(&f, &member).await?.is_some());
    assert!(!f.stripe.subscription(&subscription_id).canceled);

    travel(&f.core, period_end + 14 * DAY + 1);
    memberships::sweep(&f.core).await?;
    assert!(live(&f, &member).await?.is_none());
    assert_eq!(member_source(&f, &member).await?, None);
    // Nothing retries the charge for a membership that has ended.
    assert!(f.stripe.subscription(&subscription_id).canceled);
    Ok(())
}

#[tokio::test]
async fn a_lapse_removes_access_and_tells_the_member_how_to_renew() -> Result<()> {
    let f = fixture().await?;
    let member = pubkey();
    let membership = pay(&f, &member).await?;
    memberships::cancel_renewal(&f.core, &f.relay, &member).await?;
    let mut changes = f.core.bus.subscribe();

    travel(&f.core, membership.current_period_end.unwrap() + 1);
    memberships::sweep(&f.core).await?;

    assert!(live(&f, &member).await?.is_none());
    assert_eq!(member_source(&f, &member).await?, None);

    let now = f.core.clock.now();
    let notice = caravel_relays::notify::queries::get_notification(
        f.core.db.pool(),
        &f.relay.id,
        &member,
        now,
    )
    .await?
    .expect("no notice");
    assert!(
        notice.message.starts_with("Your membership of"),
        "{}",
        notice.message
    );
    assert_eq!(notice.expires_at, Some(now + 30 * DAY));

    // The renewal link signs the member in.
    let token = notice
        .message
        .split("token=")
        .nth(1)
        .expect("no session in the link");
    let session = sessions::find(&f.core, token).await?.expect("no session");
    assert_eq!(session.pubkey, member);

    let mut announced = false;
    while let Ok(change) = changes.try_recv() {
        announced |= matches!(change, caravel_util::bus::Change::Notified { .. });
    }
    assert!(announced);

    // Paying again clears the notice and opens a new membership.
    pay(&f, &member).await?;
    assert!(
        caravel_relays::notify::queries::get_notification(
            f.core.db.pool(),
            &f.relay.id,
            &member,
            now
        )
        .await?
        .is_none()
    );
    Ok(())
}

// Someone let in another way keeps their place when the membership they bought lapses.
#[tokio::test]
async fn a_lapse_leaves_a_member_admitted_another_way() -> Result<()> {
    let f = fixture().await?;
    let member = pubkey();
    management::add_member(&f.core, &f.relay, &member).await?;
    let membership = pay(&f, &member).await?;
    memberships::cancel_renewal(&f.core, &f.relay, &member).await?;

    travel(&f.core, membership.current_period_end.unwrap() + 1);
    memberships::sweep(&f.core).await?;

    assert!(live(&f, &member).await?.is_none());
    assert_eq!(
        member_source(&f, &member).await?,
        Some(MemberSource::Manual)
    );
    Ok(())
}

#[tokio::test]
async fn a_canceled_membership_keeps_access_until_its_period_ends() -> Result<()> {
    let f = fixture().await?;
    let member = pubkey();
    let membership = pay(&f, &member).await?;

    let canceled = memberships::cancel_renewal(&f.core, &f.relay, &member).await?;
    assert_eq!(canceled.status, MembershipStatus::Canceled);
    assert!(
        f.stripe
            .subscription(membership.stripe_subscription_id.as_deref().unwrap())
            .cancel_at_period_end
    );
    assert!(member_source(&f, &member).await?.is_some());

    // Renewing starts charging when the paid period ends.
    memberships::open_checkout(&f.core, &f.relay, &member, None).await?;
    let renewal = f.stripe.last_session();
    assert_eq!(
        renewal.form["subscription_data[billing_cycle_anchor]"],
        membership.current_period_end.unwrap().to_string()
    );
    Ok(())
}

// Ported from merchant: memberships end once their relay stops charging.
#[tokio::test]
async fn memberships_are_released_when_their_relay_stops_charging() -> Result<()> {
    let f = fixture().await?;
    let member = pubkey();
    let membership = pay(&f, &member).await?;
    let code = buy_invite(&f).await?;

    price(&f.core, f.relay.clone(), 0).await?;
    memberships::sweep(&f.core).await?;

    assert!(live(&f, &member).await?.is_none());
    assert!(
        f.stripe
            .subscription(membership.stripe_subscription_id.as_deref().unwrap())
            .canceled
    );
    // The member stays; the unused invite stops working.
    assert_eq!(
        member_source(&f, &member).await?,
        Some(MemberSource::Payment)
    );
    assert!(
        !caravel_relays::access::queries::is_live_claim(f.core.db.pool(), &f.relay.id, &code)
            .await?
    );
    Ok(())
}

#[tokio::test]
async fn a_checkout_nobody_finishes_lapses_after_two_days() -> Result<()> {
    let f = fixture().await?;
    let member = pubkey();
    memberships::open_checkout(&f.core, &f.relay, &member, None).await?;

    travel(&f.core, f.core.clock.now() + DAY);
    f.stripe.expire(&f.stripe.last_session().id);
    memberships::sweep(&f.core).await?;
    assert_eq!(
        live(&f, &member).await?.unwrap().status,
        MembershipStatus::Pending
    );

    travel(&f.core, f.core.clock.now() + 2 * DAY);
    memberships::sweep(&f.core).await?;
    assert!(live(&f, &member).await?.is_none());
    Ok(())
}

// A checkout reopened just before its membership would give up is still settled.
#[tokio::test]
async fn a_checkout_reopened_late_is_still_settled() -> Result<()> {
    let f = fixture().await?;
    let member = pubkey();
    memberships::open_checkout(&f.core, &f.relay, &member, None).await?;
    travel(&f.core, f.core.clock.now() + DAY);
    f.stripe.expire(&f.stripe.last_session().id);

    travel(&f.core, f.core.clock.now() + DAY - 60);
    memberships::open_checkout(&f.core, &f.relay, &member, None).await?;
    let reopened = f.stripe.last_session();
    travel(&f.core, f.core.clock.now() + 60 * 60);
    memberships::sweep(&f.core).await?;
    assert_eq!(
        live(&f, &member).await?.unwrap().status,
        MembershipStatus::Pending
    );

    f.stripe
        .complete(&reopened.id, f.core.clock.now() + 30 * DAY);
    memberships::sweep(&f.core).await?;
    assert_eq!(
        live(&f, &member).await?.unwrap().status,
        MembershipStatus::Active
    );
    Ok(())
}

// A checkout paid without coming back to the page is settled by the sweep.
#[tokio::test]
async fn the_sweep_settles_a_checkout_its_payer_left() -> Result<()> {
    let f = fixture().await?;
    let member = pubkey();
    memberships::open_checkout(&f.core, &f.relay, &member, None).await?;
    let session = f.stripe.last_session();
    f.stripe
        .complete(&session.id, f.core.clock.now() + 30 * DAY);

    memberships::sweep(&f.core).await?;
    assert_eq!(
        live(&f, &member).await?.unwrap().status,
        MembershipStatus::Active
    );
    assert_eq!(
        member_source(&f, &member).await?,
        Some(MemberSource::Payment)
    );
    Ok(())
}

// Ported from zooid's payments validation: a price Stripe can charge, on a relay
// that doesn't let anyone in for free.
#[tokio::test]
async fn a_member_price_must_be_chargeable_and_need_a_claim() -> Result<()> {
    let f = fixture().await?;

    let cheap = RelaySettings {
        member_price: Some(10),
        ..Default::default()
    };
    let cheap = hosting::update_relay(&f.core, f.relay.clone(), cheap).await;
    assert!(matches!(
        cheap,
        Err(Error::Invalid {
            code: "invalid-member-price",
            ..
        })
    ));

    let open = RelaySettings {
        policy_public_join: Some(caravel_util::flag::Flag(true)),
        ..Default::default()
    };
    let open = hosting::update_relay(&f.core, f.relay.clone(), open).await;
    assert!(matches!(
        open,
        Err(Error::Invalid {
            code: "paid-public-join",
            ..
        })
    ));

    // A reader who authenticates must never be worse off than one who doesn't.
    let anonymous = RelaySettings {
        policy_anonymous_read: Some(caravel_util::flag::Flag(true)),
        ..Default::default()
    };
    let anonymous = hosting::update_relay(&f.core, f.relay.clone(), anonymous).await;
    assert!(matches!(
        anonymous,
        Err(Error::Invalid {
            code: "anonymous-read-needs-public-read",
            ..
        })
    ));
    Ok(())
}

/// A plan change may not leave more members on the relay than the new plan allows.
#[tokio::test]
async fn a_plan_with_fewer_seats_than_the_relay_has_members_is_refused() -> Result<()> {
    let core = core().await;
    let tenant = create_tenant(&core).await?;
    let relay = create_relay(&core, &tenant, PLAN_BASIC).await?;

    // Basic allows a hundred, so the owner plus eleven fits; free allows ten.
    for _ in 0..11 {
        let member = SecretKey::generate();
        management::add_member(&core, &relay, &member.public_key().to_hex()).await?;
    }

    let smaller = RelaySettings {
        plan_id: Some(PLAN_FREE.into()),
        blossom_enabled: Some(caravel_util::flag::Flag(false)),
        livekit_enabled: Some(caravel_util::flag::Flag(false)),
        ..Default::default()
    };
    let refused = hosting::update_relay(&core, relay, smaller).await;
    assert!(matches!(
        refused,
        Err(Error::Invalid {
            code: "member-limit-exceeded",
            ..
        })
    ));
    Ok(())
}

#[tokio::test]
async fn an_offer_says_whether_payment_is_taken() -> Result<()> {
    let f = fixture().await?;

    let offer = memberships::get_offer(&f.core, &f.relay).await?;
    assert_eq!((offer.price, offer.accepting_payment), (500, true));

    caravel_billing::tenants::commands::set_tenant_stripe_account(
        f.core.db.pool(),
        &f.relay.tenant_pubkey,
        "acct_ready",
        false,
    )
    .await?;
    assert!(
        !memberships::get_offer(&f.core, &f.relay)
            .await?
            .accepting_payment
    );
    Ok(())
}

#[tokio::test]
async fn a_tenant_onboards_for_payouts() -> Result<()> {
    let f = fixture().await?;
    let tenant_pubkey = create_tenant(&f.core).await?;
    let tenant = get_tenant(&f.core, &tenant_pubkey).await?;

    let status = caravel_core::payouts::refresh_status(&f.core, &tenant).await?;
    assert_eq!((status.account_id, status.ready), (None, false));

    let link = caravel_core::payouts::onboard(&f.core, &tenant, "https://app.test/account").await?;
    let tenant = get_tenant(&f.core, &tenant_pubkey).await?;
    assert_eq!(
        link,
        format!(
            "https://connect.stripe.test/onboard/{}",
            tenant.stripe_account_id.as_deref().unwrap()
        )
    );

    f.stripe
        .set_account_ready(tenant.stripe_account_id.as_deref().unwrap(), true);
    let status = caravel_core::payouts::refresh_status(&f.core, &tenant).await?;
    assert!(status.ready);
    assert!(
        get_tenant(&f.core, &tenant_pubkey)
            .await?
            .stripe_account_ready
    );
    Ok(())
}

// Erasing a relay leaves paying members in, and paid invites usable.
#[tokio::test]
async fn erasing_a_relay_keeps_what_was_paid_for() -> Result<()> {
    let f = fixture().await?;
    let paying = pubkey();
    pay(&f, &paying).await?;
    let code = buy_invite(&f).await?;
    let added = pubkey();
    management::add_member(&f.core, &f.relay, &added).await?;

    hosting::erase_relay(&f.core, &f.relay).await?;

    assert_eq!(
        member_source(&f, &paying).await?,
        Some(MemberSource::Payment)
    );
    assert_eq!(member_source(&f, &added).await?, None);

    let joiner = SecretKey::generate();
    join(&f.core, &f.relay, &joiner, &code).await?;
    assert_eq!(
        member_source(&f, &joiner.public_key().to_hex()).await?,
        Some(MemberSource::Payment)
    );
    Ok(())
}
