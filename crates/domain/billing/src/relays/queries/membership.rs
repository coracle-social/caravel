//! Reading paid memberships, and the seats they hold.

use sqlx::{PgConnection, PgExecutor};

use caravel_util::error::{Error, Result};

use crate::relays::{Membership, MembershipStatus, Relay};

const SELECT_MEMBERSHIP: &str =
    "SELECT id, relay_id, pubkey, invite_code, stripe_customer_id, stripe_subscription_id,
    status, current_period_end, bound_at, created_at FROM membership";

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

/// Live memberships that should no longer be paid for: those of relays that stopped
/// selling them, and those of pubkeys no longer on their relay.
pub async fn list_memberships_needing_release(db: impl PgExecutor<'_>) -> Result<Vec<Membership>> {
    Ok(sqlx::query_as::<_, Membership>(&format!(
        "{SELECT_MEMBERSHIP} WHERE status <> 'lapsed' AND (
           relay_id IN (
             SELECT id FROM relay WHERE member_price = 0 OR status <> 'active' OR purged_at IS NOT NULL
           )
           OR (status IN ('active', 'canceled') AND pubkey IS NOT NULL AND NOT EXISTS (
             SELECT 1 FROM relay_member
             WHERE relay_member.relay_id = membership.relay_id
               AND relay_member.pubkey = membership.pubkey
           ))
         )"
    ))
    .fetch_all(db)
    .await?)
}

pub async fn list_memberships_with_status(
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

/// The most invites one relay may have in flight at once, for a plan that sets no
/// member limit: an anonymous checkout still has to be bounded by something.
pub const MAX_INVITES_IN_FLIGHT: i64 = 100;

/// Whether a relay has room for one more paying member. A pubkey already on the
/// relay takes no new seat, and each invite paid for and unused holds one.
pub async fn has_seat(
    conn: &mut PgConnection,
    relay: &Relay,
    pubkey: Option<&str>,
) -> Result<bool> {
    let Some(limit) = relay.member_limit() else {
        return Ok(true);
    };

    if let Some(pubkey) = pubkey
        && caravel_relays::access::queries::get_member(&mut *conn, &relay.id, pubkey)
            .await?
            .is_some()
    {
        return Ok(true);
    }

    let members = caravel_relays::access::queries::count_members(&mut *conn, &relay.id).await?;
    let reserved = count_reserved_seats(conn, &relay.id).await?;
    Ok(members + reserved < limit)
}

/// Refuse one more member the relay's plan has no seat for.
pub async fn require_seat(
    conn: &mut PgConnection,
    relay: &Relay,
    pubkey: Option<&str>,
) -> Result<()> {
    if has_seat(conn, relay, pubkey).await? {
        return Ok(());
    }
    Err(Error::conflict(
        "member-limit",
        "this relay has reached its member limit",
    ))
}

/// Whether the relay can open one more invite. An invite nobody has paid for yet
/// holds its seat until it lapses, so opening checkouts can't outnumber the seats
/// the relay has to honor them with; a plan with no member limit is held to
/// [`MAX_INVITES_IN_FLIGHT`] instead.
pub async fn has_invite_seat(conn: &mut PgConnection, relay: &Relay) -> Result<bool> {
    let in_flight = count_invites_in_flight(&mut *conn, &relay.id).await?;
    let Some(limit) = relay.member_limit() else {
        return Ok(in_flight < MAX_INVITES_IN_FLIGHT);
    };

    let members = caravel_relays::access::queries::count_members(&mut *conn, &relay.id).await?;
    Ok(members + in_flight < limit)
}

/// A relay's invites with no pubkey yet: those opened and unpaid, and those paid
/// for and waiting for whoever joins with their code.
async fn count_invites_in_flight(db: impl PgExecutor<'_>, relay_id: &str) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM membership
         WHERE relay_id = $1 AND pubkey IS NULL AND status IN ('pending', 'active', 'canceled')",
    )
    .bind(relay_id)
    .fetch_one(db)
    .await?)
}
