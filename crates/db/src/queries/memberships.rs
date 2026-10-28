//! Reading paid memberships, and the seats they hold.

use sqlx::{PgConnection, PgExecutor};

use crate::queries;
use caravel_util::error::Result;
use caravel_domain::memberships::{Membership, MembershipStatus, RelayMember};
use caravel_domain::relays::Relay;

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
        && queries::memberships::get_member(&mut *conn, &relay.id, pubkey)
            .await?
            .is_some()
    {
        return Ok(true);
    }

    let members = queries::memberships::count_members(&mut *conn, &relay.id).await?;
    let reserved = count_reserved_seats(conn, &relay.id).await?;
    Ok(members + reserved < limit)
}

pub async fn list_members(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<RelayMember>> {
    Ok(sqlx::query_as::<_, RelayMember>(
        "SELECT pubkey, source, roles, created_at FROM relay_member
         WHERE relay_id = $1 ORDER BY created_at, pubkey",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

pub async fn get_member(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    pubkey: &str,
) -> Result<Option<RelayMember>> {
    Ok(sqlx::query_as::<_, RelayMember>(
        "SELECT pubkey, source, roles, created_at FROM relay_member WHERE relay_id = $1 AND pubkey = $2",
    )
    .bind(relay_id)
    .bind(pubkey)
    .fetch_optional(db)
    .await?)
}

pub async fn count_members(db: impl PgExecutor<'_>, relay_id: &str) -> Result<i64> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM relay_member WHERE relay_id = $1")
            .bind(relay_id)
            .fetch_one(db)
            .await?,
    )
}
