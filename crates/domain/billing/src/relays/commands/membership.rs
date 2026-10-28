//! Writing paid memberships, and admitting the NIP-43 joins that can bind one.

use coracle_lib::events::Event;
use sqlx::{PgConnection, PgExecutor};

use caravel_relays::access;
use caravel_relays::access::MemberSource;
use caravel_relays::policy::Join;
use caravel_util::error::{Error, Result};
use caravel_util::nostr::Key;

use super::super::queries;
use crate::relays::{Membership, MembershipStatus, Relay};

pub async fn insert_membership(db: impl PgExecutor<'_>, membership: &Membership) -> Result<()> {
    sqlx::query(
        "INSERT INTO membership (id, relay_id, pubkey, invite_code, stripe_customer_id, stripe_subscription_id,
           status, current_period_end, bound_at, created_at)
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
    .bind(membership.bound_at)
    .bind(membership.created_at)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_customer(db: impl PgExecutor<'_>, id: &str, customer_id: &str) -> Result<()> {
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

/// Admit a NIP-43 join, under a lock on the relay so racing joins see each other.
/// A claim must still be live. A paid invite's claim gives its membership to the
/// joiner, unless they hold one already, and stops working; the joiner is then a
/// paying member. A new member must fit within the plan's member limit, counting
/// the seats unused invites hold. Returns what the relay published.
pub async fn admit_join(
    conn: &mut PgConnection,
    key: &Key,
    relay: &Relay,
    event: &Event,
    join: &Join,
    now: i64,
) -> Result<Vec<Event>> {
    let relay_id = relay.id.as_str();
    let pubkey = event.pubkey.to_hex();
    let pubkey = pubkey.as_str();
    super::super::queries::lock_relay(&mut *conn, relay_id).await?;

    let source = match join {
        Join::AlreadyMember => None,
        Join::Open => Some(MemberSource::PublicJoin),
        Join::Claim(code) => {
            if !caravel_relays::access::queries::is_live_claim(&mut *conn, relay_id, code).await? {
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

    let Some(source) = source else {
        return Ok(Vec::new());
    };
    queries::membership::require_seat(&mut *conn, relay, Some(pubkey)).await?;
    access::commands::admit(conn, key, relay_id, pubkey, source, now).await
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
    if queries::membership::get_live_membership(&mut *conn, relay_id, pubkey)
        .await?
        .is_some()
    {
        return Ok(false);
    }

    let bound = sqlx::query(
        "UPDATE membership SET pubkey = $3, bound_at = $4
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

    caravel_relays::access::commands::revoke_claim(conn, relay_id, code, now).await?;
    Ok(true)
}

/// Record that a membership pays through a subscription, with its status and the
/// end of the period paid for.
pub async fn set_paid(
    db: impl PgExecutor<'_>,
    id: &str,
    status: MembershipStatus,
    period_end: i64,
    customer_id: &str,
    subscription_id: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE membership SET status = $2, current_period_end = $3, stripe_customer_id = $4,
           stripe_subscription_id = $5
         WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(period_end)
    .bind(customer_id)
    .bind(subscription_id)
    .execute(db)
    .await?;
    Ok(())
}
