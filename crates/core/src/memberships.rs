//! Paid memberships: members paying a relay's tenant, through Stripe Connect, for
//! access to the relay. A membership drives the relay's access list in the
//! transaction that changes it. Paying admits its member, or registers its invite
//! as a claim for whoever joins with it; lapsing removes a member who got in by
//! paying, and tells them why. A sweep keeps every membership in step with Stripe.

use caravel_adapters::stripe::MembershipCheckoutParams;
use coracle_lib::events::Event;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgExecutor};

use crate::access::{self, MemberSource};
use crate::bus::Change;
use crate::crypto::RelayKey;
use crate::error::{Error, Result};
use crate::relay_policy::Join;
use crate::relays::{self, MemberInterval, Relay};
use crate::{Core, auth, notifications, plans, tenants};

/// How long a lapse notice, and the session its link carries, stays good.
const NOTICE_TTL: i64 = 30 * 24 * 60 * 60;

/// A checkout not completed in this long never will be: Stripe expires Checkout
/// sessions after a day.
const PENDING_TTL: i64 = 2 * 24 * 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "text", rename_all = "lowercase")]
pub enum MembershipStatus {
    /// A checkout was opened and hasn't been paid.
    Pending,
    /// Paid for, and renewing.
    Active,
    /// Paid for until its period ends, and not renewing.
    Canceled,
    /// Over. A lapsed membership is history; paying again opens a new one.
    Lapsed,
}

impl MembershipStatus {
    /// Whether the membership is paid for.
    pub fn is_paid(self) -> bool {
        matches!(self, MembershipStatus::Active | MembershipStatus::Canceled)
    }
}

/// One member's paid access to one relay. A membership bought without a pubkey
/// is an invite: its code is registered as a claim once paid, and the membership
/// goes to the first pubkey without one of its own that joins with it.
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Membership {
    pub id: String,
    pub relay_id: String,
    /// The member, or `None` for an invite nobody has used yet.
    pub pubkey: Option<String>,
    /// The code of a membership bought as an invite.
    pub invite_code: Option<String>,
    /// The Stripe customer the membership pays as, or empty before checkout.
    pub stripe_customer_id: String,
    /// The subscription paying for it, or empty before it is paid.
    pub stripe_subscription_id: String,
    pub status: MembershipStatus,
    /// When the period paid for ends.
    pub current_period_end: Option<i64>,
    /// When an invite's membership went to the pubkey that joined with it.
    pub claimed_at: Option<i64>,
    pub created_at: i64,
}

/// What anyone may see of a relay that sells memberships.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Offer {
    pub relay_id: String,
    pub host: String,
    pub name: String,
    pub icon: String,
    pub description: String,
    /// In cents per `interval`.
    pub price: i64,
    pub interval: MemberInterval,
    /// Whether the tenant can take payments now.
    pub accepting_payment: bool,
}

/// What an invite's buyer sees: its relay, whether it is paid for and used, and
/// the link that accepts it while it is paid for and unused.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InviteView {
    pub offer: Offer,
    pub status: MembershipStatus,
    pub claimed: bool,
    pub invite_url: Option<String>,
}

// --- Reads ---

const SELECT_MEMBERSHIP: &str =
    "SELECT id, relay_id, pubkey, invite_code, stripe_customer_id, stripe_subscription_id,
    status, current_period_end, claimed_at, created_at FROM membership";

/// A pubkey's membership of a relay that hasn't lapsed.
pub async fn get_live_membership(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    pubkey: &str,
) -> Result<Option<Membership>> {
    Ok(sqlx::query_as::<_, Membership>(&format!(
        "{SELECT_MEMBERSHIP} WHERE relay_id = $1 AND pubkey = $2 AND status <> 'lapsed'"
    ))
    .bind(relay_id)
    .bind(pubkey)
    .fetch_optional(db)
    .await?)
}

pub async fn get_membership_by_invite(
    db: impl PgExecutor<'_>,
    code: &str,
) -> Result<Option<Membership>> {
    Ok(
        sqlx::query_as::<_, Membership>(&format!("{SELECT_MEMBERSHIP} WHERE invite_code = $1"))
            .bind(code)
            .fetch_optional(db)
            .await?,
    )
}

/// Every membership a pubkey holds that hasn't lapsed.
pub async fn list_live_memberships_for_pubkey(
    db: impl PgExecutor<'_>,
    pubkey: &str,
) -> Result<Vec<Membership>> {
    Ok(sqlx::query_as::<_, Membership>(&format!(
        "{SELECT_MEMBERSHIP} WHERE pubkey = $1 AND status <> 'lapsed' ORDER BY created_at DESC"
    ))
    .bind(pubkey)
    .fetch_all(db)
    .await?)
}

/// A relay's memberships, newest first.
pub async fn list_memberships_for_relay(
    db: impl PgExecutor<'_>,
    relay_id: &str,
) -> Result<Vec<Membership>> {
    Ok(sqlx::query_as::<_, Membership>(&format!(
        "{SELECT_MEMBERSHIP} WHERE relay_id = $1 ORDER BY created_at DESC"
    ))
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

/// Invites paid for and not yet used, each holding a seat on its relay.
pub async fn count_reserved_seats(db: impl PgExecutor<'_>, relay_id: &str) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM membership
         WHERE relay_id = $1 AND pubkey IS NULL AND status IN ('active', 'canceled')",
    )
    .bind(relay_id)
    .fetch_one(db)
    .await?)
}

/// Live memberships of relays that no longer sell them: free, paused or purged.
pub async fn list_memberships_needing_release(db: impl PgExecutor<'_>) -> Result<Vec<Membership>> {
    Ok(sqlx::query_as::<_, Membership>(&format!(
        "{SELECT_MEMBERSHIP} WHERE status <> 'lapsed' AND relay_id IN (
           SELECT id FROM relay WHERE member_price = 0 OR status <> 'active' OR purged_at IS NOT NULL
         )"
    ))
    .fetch_all(db)
    .await?)
}

async fn list_memberships_with_status(
    db: impl PgExecutor<'_>,
    status: MembershipStatus,
) -> Result<Vec<Membership>> {
    Ok(
        sqlx::query_as::<_, Membership>(&format!("{SELECT_MEMBERSHIP} WHERE status = $1"))
            .bind(status)
            .fetch_all(db)
            .await?,
    )
}

/// Paid memberships whose period has ended by `now`.
pub async fn list_memberships_past_period(
    db: impl PgExecutor<'_>,
    now: i64,
) -> Result<Vec<Membership>> {
    Ok(sqlx::query_as::<_, Membership>(&format!(
        "{SELECT_MEMBERSHIP} WHERE status IN ('active', 'canceled') AND current_period_end <= $1"
    ))
    .bind(now)
    .fetch_all(db)
    .await?)
}

// --- Writes ---

pub async fn insert_membership(db: impl PgExecutor<'_>, membership: &Membership) -> Result<()> {
    sqlx::query(
        "INSERT INTO membership (id, relay_id, pubkey, invite_code, stripe_customer_id, stripe_subscription_id,
           status, current_period_end, claimed_at, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(&membership.id)
    .bind(&membership.relay_id)
    .bind(&membership.pubkey)
    .bind(&membership.invite_code)
    .bind(&membership.stripe_customer_id)
    .bind(&membership.stripe_subscription_id)
    .bind(membership.status)
    .bind(membership.current_period_end)
    .bind(membership.claimed_at)
    .bind(membership.created_at)
    .execute(db)
    .await?;
    Ok(())
}

async fn set_customer(db: impl PgExecutor<'_>, id: &str, customer_id: &str) -> Result<()> {
    sqlx::query("UPDATE membership SET stripe_customer_id = $2 WHERE id = $1")
        .bind(id)
        .bind(customer_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Move a membership to `status`, and to a new period end if one is given.
pub async fn set_status(
    db: impl PgExecutor<'_>,
    id: &str,
    status: MembershipStatus,
    period_end: Option<i64>,
) -> Result<()> {
    sqlx::query(
        "UPDATE membership SET status = $2, current_period_end = COALESCE($3, current_period_end) WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(period_end)
    .execute(db)
    .await?;
    Ok(())
}

// --- Seats and joins ---

/// Whether a relay has room for one more paying member. A pubkey already on the
/// relay takes no new seat, and each invite paid for and unused holds one.
pub async fn has_seat(
    conn: &mut PgConnection,
    relay: &Relay,
    pubkey: Option<&str>,
) -> Result<bool> {
    let Some(limit) = plans::get_plan(&relay.plan_id).and_then(|plan| plan.members) else {
        return Ok(true);
    };

    if let Some(pubkey) = pubkey
        && access::get_member(&mut *conn, &relay.id, pubkey)
            .await?
            .is_some()
    {
        return Ok(true);
    }

    let members = access::count_members(&mut *conn, &relay.id).await?;
    let reserved = count_reserved_seats(conn, &relay.id).await?;
    Ok(members + reserved < limit)
}

/// Admit a NIP-43 join, under a lock on the relay so racing joins see each other.
/// A claim must still be live. A paid invite's claim gives its membership to the
/// joiner, unless they hold one already, and stops working; the joiner is then a
/// paying member. A new member must fit within the plan's member limit, counting
/// the seats unused invites hold. Every join presenting a claim is logged against
/// it. Returns what the relay published.
pub async fn admit_join(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    event: &Event,
    join: &Join,
    limit: Option<usize>,
    now: i64,
) -> Result<Vec<Event>> {
    let pubkey = event.pubkey.to_hex();
    let pubkey = pubkey.as_str();
    sqlx::query("SELECT 1 FROM relay WHERE id = $1 FOR UPDATE")
        .bind(relay_id)
        .execute(&mut *conn)
        .await?;

    let source = match join {
        Join::AlreadyMember => None,
        Join::Open => Some(MemberSource::PublicJoin),
        Join::Claim(code) => {
            if !access::is_live_claim(&mut *conn, relay_id, code).await? {
                return Err(Error::conflict(
                    "invalid-claim",
                    "failed to validate invite code",
                ));
            }
            if bind_invite(&mut *conn, relay_id, code, pubkey, now).await? {
                Some(MemberSource::Payment)
            } else {
                Some(MemberSource::Claim)
            }
        }
    };

    let mut published = Vec::new();
    if let Some(source) = source {
        if let Some(limit) = limit
            && access::get_member(&mut *conn, relay_id, pubkey)
                .await?
                .is_none()
        {
            let taken = access::count_members(&mut *conn, relay_id).await?
                + count_reserved_seats(&mut *conn, relay_id).await?;
            if taken >= limit as i64 {
                return Err(Error::conflict(
                    "member-limit",
                    "this relay has reached its member limit",
                ));
            }
        }
        published = access::admit(&mut *conn, key, relay_id, pubkey, source, now).await?;
    }

    if let Some(code) = event.tags.value("claim") {
        access::record_claim_use(conn, relay_id, code, pubkey, now).await?;
    }

    Ok(published)
}

/// Give a paid, unused invite's membership to `pubkey`, if it holds no membership
/// of the relay already, and revoke the invite's claim. Returns whether it did.
async fn bind_invite(
    conn: &mut PgConnection,
    relay_id: &str,
    code: &str,
    pubkey: &str,
    now: i64,
) -> Result<bool> {
    if get_live_membership(&mut *conn, relay_id, pubkey)
        .await?
        .is_some()
    {
        return Ok(false);
    }

    let bound = sqlx::query(
        "UPDATE membership SET pubkey = $3, claimed_at = $4
         WHERE relay_id = $1 AND invite_code = $2 AND pubkey IS NULL AND status IN ('active', 'canceled')",
    )
    .bind(relay_id)
    .bind(code)
    .bind(pubkey)
    .bind(now)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    if bound == 0 {
        return Ok(false);
    }

    access::revoke_claim(conn, relay_id, code, now).await?;
    Ok(true)
}

// --- Pages ---

/// A page of the relay's member portal, on its own host.
pub fn portal_url(relay: &Relay, path: &str) -> String {
    format!("https://{}{path}", relay.host())
}

/// Where an invite is accepted: Flotilla's join page for the relay and code.
pub fn invite_url(core: &Core, relay: &Relay, code: &str) -> Result<String> {
    let mut url =
        url::Url::parse(&format!("{}/join", core.config.flotilla_url)).map_err(Error::internal)?;
    url.query_pairs_mut()
        .append_pair("r", &relay.host())
        .append_pair("c", code);
    Ok(url.to_string())
}

/// `page`, carrying the `return_url` a client asked to be sent back to. The portal
/// only offers it as a link, so it needs no allow-list.
fn with_return(page: &str, return_url: Option<&str>) -> Result<String> {
    let mut url = url::Url::parse(page).map_err(Error::internal)?;
    if let Some(back) = return_url.map(str::trim).filter(|url| !url.is_empty()) {
        url.query_pairs_mut().append_pair("return_url", back);
    }
    Ok(url.to_string())
}

/// Where Stripe sends a payer back to: `page`, with the session for it to settle.
/// Stripe fills in the placeholder, so it is appended unencoded.
fn checkout_success_url(page: &str) -> String {
    let separator = if page.contains('?') { '&' } else { '?' };
    format!("{page}{separator}session={{CHECKOUT_SESSION_ID}}")
}

/// What a relay sells memberships for, and whether it can take payment now.
pub async fn get_offer(core: &Core, relay: &Relay) -> Result<Offer> {
    let accepting_payment = tenants::get_tenant(core.db.pool(), &relay.tenant_pubkey)
        .await?
        .is_some_and(|tenant| tenant.stripe_account_ready);

    Ok(Offer {
        relay_id: relay.id.clone(),
        host: relay.host(),
        name: relay.display_name(),
        icon: relay.info_icon.clone(),
        description: relay.info_description.clone(),
        price: relay.member_price,
        interval: relay.member_interval,
        accepting_payment: accepting_payment && relay.charges_members(),
    })
}

/// An invite as its buyer sees it.
pub async fn get_invite(core: &Core, code: &str) -> Result<InviteView> {
    let membership = get_membership_by_invite(core.db.pool(), code)
        .await?
        .ok_or(Error::NotFound("invite not found"))?;
    let relay = relays::get_relay(core.db.pool(), &membership.relay_id)
        .await?
        .ok_or(Error::NotFound("invite not found"))?;
    let offer = get_offer(core, &relay).await?;

    let claimed = membership.pubkey.is_some();
    let invite_url = match membership.status.is_paid() && !claimed {
        true => Some(invite_url(core, &relay, code)?),
        false => None,
    };

    Ok(InviteView {
        offer,
        status: membership.status,
        claimed,
        invite_url,
    })
}

// --- Checkout ---

/// The tenant's Connect account, if it can take payments.
async fn payout_account(core: &Core, relay: &Relay) -> Result<String> {
    tenants::get_tenant(core.db.pool(), &relay.tenant_pubkey)
        .await?
        .filter(|tenant| tenant.stripe_account_ready && !tenant.stripe_account_id.is_empty())
        .map(|tenant| tenant.stripe_account_id)
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
            let live = get_live_membership(&mut *conn, &relay.id, pubkey).await?;
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
            if !has_seat(&mut *conn, relay, Some(pubkey)).await? {
                return Err(Error::conflict("member-limit", "this relay is full"));
            }
            if let Some(live) = live {
                return Ok(live);
            }

            let membership = new_membership(&relay.id, Some(pubkey), None, now);
            insert_membership(conn, &membership).await?;
            Ok(membership)
        })
        .await?;

    let page = with_return(&portal_url(relay, "/"), return_url)?;
    checkout(
        core,
        relay,
        &account,
        &membership,
        &checkout_success_url(&page),
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
            if !has_seat(&mut *conn, relay, None).await? {
                return Err(Error::conflict("member-limit", "this relay is full"));
            }
            let membership = new_membership(&relay.id, None, Some(&code), now);
            insert_membership(conn, &membership).await?;
            Ok(membership)
        })
        .await?;

    let success = with_return(&portal_url(relay, &format!("/invites/{code}")), return_url)?;
    let cancel = with_return(&portal_url(relay, "/"), return_url)?;
    checkout(
        core,
        relay,
        &account,
        &membership,
        &checkout_success_url(&success),
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
        stripe_customer_id: String::new(),
        stripe_subscription_id: String::new(),
        status: MembershipStatus::Pending,
        current_period_end: None,
        claimed_at: None,
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
    let customer_id = if membership.stripe_customer_id.is_empty() {
        let customer_id = core
            .stripe
            .create_member_customer(&membership.id, &relay.id)
            .await?;
        set_customer(core.db.pool(), &membership.id, &customer_id).await?;
        customer_id
    } else {
        membership.stripe_customer_id.clone()
    };

    // A canceled membership has a period it already paid for, so renewing starts
    // charging when that ends rather than now.
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
                .unwrap_or_else(|| membership.stripe_customer_id.clone());
            activate(core, membership, &customer_id, &subscription_id).await
        }
        _ => Ok(false),
    }
}

/// Record that a membership pays through `subscription_id` and grant it access:
/// admit its member, or register its invite's claim. Returns whether the
/// subscription has a paid period still running.
pub async fn activate(
    core: &Core,
    membership: &Membership,
    customer_id: &str,
    subscription_id: &str,
) -> Result<bool> {
    let subscription = core.stripe.get_subscription(subscription_id).await?;
    let now = core.clock.now();
    let Some(period_end) = subscription.current_period_end.filter(|end| *end > now) else {
        return Ok(false);
    };
    let status = if subscription.cancel_at_period_end {
        MembershipStatus::Canceled
    } else {
        MembershipStatus::Active
    };

    let relay = load_relay(core, &membership.relay_id).await?;
    let key = RelayKey::decrypt(&core.config.platform_key, &relay.secret)?;
    let published = core
        .db
        .with_tx(async |conn| {
            sqlx::query(
                "UPDATE membership SET status = $2, current_period_end = $3, stripe_customer_id = $4,
                   stripe_subscription_id = $5
                 WHERE id = $1",
            )
            .bind(&membership.id)
            .bind(status)
            .bind(period_end)
            .bind(customer_id)
            .bind(subscription_id)
            .execute(&mut *conn)
            .await?;

            match (&membership.pubkey, &membership.invite_code) {
                (Some(pubkey), _) => {
                    notifications::clear(&mut *conn, &relay.id, pubkey).await?;
                    access::admit(conn, &key, &relay.id, pubkey, MemberSource::Payment, now).await
                }
                (None, Some(code)) => {
                    access::create_claim(conn, &relay.id, code, now).await?;
                    Ok(Vec::new())
                }
                (None, None) => Ok(Vec::new()),
            }
        })
        .await?;

    publish_access_change(core, &relay.id, published);
    Ok(true)
}

/// Stop a member's membership of a relay from renewing. It keeps its access until
/// its paid period ends.
pub async fn cancel_renewal(core: &Core, relay: &Relay, pubkey: &str) -> Result<Membership> {
    let membership = get_live_membership(core.db.pool(), &relay.id, pubkey)
        .await?
        .filter(|m| m.status == MembershipStatus::Active)
        .ok_or(Error::NotFound("membership not found"))?;

    core.stripe
        .cancel_subscription_at_period_end(&membership.stripe_subscription_id)
        .await?;
    set_status(
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
    let key = RelayKey::decrypt(&core.config.platform_key, &relay.secret)?;
    let now = core.clock.now();
    let published = core
        .db
        .with_tx(async |conn| {
            set_status(&mut *conn, &membership.id, MembershipStatus::Lapsed, None).await?;
            match (&membership.pubkey, &membership.invite_code) {
                (Some(pubkey), _) => {
                    let paid = access::get_member(&mut *conn, &relay.id, pubkey)
                        .await?
                        .is_some_and(|member| member.source == MemberSource::Payment);
                    if paid {
                        access::remove(conn, &key, &relay.id, &relay.tenant_pubkey, pubkey, now)
                            .await
                    } else {
                        Ok(Vec::new())
                    }
                }
                (None, Some(code)) => {
                    access::revoke_claim(conn, &relay.id, code, now).await?;
                    Ok(Vec::new())
                }
                (None, None) => Ok(Vec::new()),
            }
        })
        .await?;
    publish_access_change(core, &relay.id, published);

    if let Some(pubkey) = &membership.pubkey {
        let (token, _) = auth::create_session(core.db.pool(), pubkey, now).await?;
        let mut renew = url::Url::parse(&portal_url(relay, "/")).map_err(Error::internal)?;
        renew.query_pairs_mut().append_pair("session", &token);
        let message = format!(
            "Your membership of {} has ended. Renew at {renew}",
            relay.display_name()
        );
        notifications::notify(core, &relay.id, pubkey, &message, Some(now + NOTICE_TTL)).await?;
    }

    Ok(())
}

/// End a membership whose relay stopped selling them. Nothing more is charged and
/// nothing is refunded; a member stays on the relay, and an unused invite stops
/// working.
async fn release(core: &Core, membership: &Membership) -> Result<()> {
    if !membership.stripe_customer_id.is_empty() {
        for session_id in core
            .stripe
            .list_open_checkout_sessions(&membership.stripe_customer_id)
            .await?
        {
            core.stripe.expire_checkout_session(&session_id).await?;
        }
        for subscription_id in core
            .stripe
            .list_live_subscriptions(&membership.stripe_customer_id)
            .await?
        {
            core.stripe.cancel_subscription(&subscription_id).await?;
        }
    }

    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            set_status(&mut *conn, &membership.id, MembershipStatus::Lapsed, None).await?;
            if membership.pubkey.is_none()
                && let Some(code) = &membership.invite_code
            {
                access::revoke_claim(conn, &membership.relay_id, code, now).await?;
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
    for membership in list_memberships_needing_release(core.db.pool()).await? {
        if let Err(error) = release(core, &membership).await {
            tracing::warn!(membership = %membership.id, error = %error, "failed to release a membership");
        }
    }

    for membership in
        list_memberships_with_status(core.db.pool(), MembershipStatus::Pending).await?
    {
        if let Err(error) = settle_pending(core, &membership).await {
            tracing::warn!(membership = %membership.id, error = %error, "failed to settle a checkout");
        }
    }

    for membership in list_memberships_past_period(core.db.pool(), core.clock.now()).await? {
        if let Err(error) = renew_or_lapse(core, &membership).await {
            tracing::warn!(membership = %membership.id, error = %error, "failed to settle a membership");
        }
    }

    Ok(())
}

/// Activate a membership whose checkout completed without its payer coming back,
/// or give up on one that never will.
async fn settle_pending(core: &Core, membership: &Membership) -> Result<()> {
    if membership.created_at < core.clock.now() - PENDING_TTL {
        return set_status(
            core.db.pool(),
            &membership.id,
            MembershipStatus::Lapsed,
            None,
        )
        .await;
    }

    if membership.stripe_customer_id.is_empty() {
        return Ok(());
    }

    if let Some(subscription_id) = core
        .stripe
        .list_live_subscriptions(&membership.stripe_customer_id)
        .await?
        .first()
    {
        activate(
            core,
            membership,
            &membership.stripe_customer_id,
            subscription_id,
        )
        .await?;
    }
    Ok(())
}

/// Ask Stripe where a membership whose period ended stands: a renewed period
/// carries on, a renewal paid in a newer checkout takes over, and anything else
/// lapses it.
async fn renew_or_lapse(core: &Core, membership: &Membership) -> Result<()> {
    let relay = load_relay(core, &membership.relay_id).await?;
    let now = core.clock.now();

    let subscription = core
        .stripe
        .get_subscription(&membership.stripe_subscription_id)
        .await?;
    if let Some(period_end) = subscription.current_period_end.filter(|end| *end > now) {
        let status = if subscription.cancel_at_period_end {
            MembershipStatus::Canceled
        } else {
            MembershipStatus::Active
        };
        return set_status(core.db.pool(), &membership.id, status, Some(period_end)).await;
    }

    let renewal = core
        .stripe
        .list_live_subscriptions(&membership.stripe_customer_id)
        .await?
        .into_iter()
        .find(|id| *id != membership.stripe_subscription_id);
    if let Some(subscription_id) = renewal
        && activate(
            core,
            membership,
            &membership.stripe_customer_id,
            &subscription_id,
        )
        .await?
    {
        return Ok(());
    }

    lapse(core, membership, &relay).await
}

async fn load_relay(core: &Core, relay_id: &str) -> Result<Relay> {
    relays::get_relay(core.db.pool(), relay_id)
        .await?
        .ok_or(Error::NotFound("relay not found"))
}

fn publish_access_change(core: &Core, relay_id: &str, published: Vec<Event>) {
    core.bus.publish(Change::AccessChanged {
        relay_id: relay_id.to_string(),
        published,
    });
}
