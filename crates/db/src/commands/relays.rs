//! Writing relays: their rows, the hosts that redirect to them, and erasing what
//! they hold.

use coracle_lib::events::Event;
use sqlx::{PgConnection, PgExecutor};

use crate::commands;
use crate::queries;
use caravel_domain::activity::ActivityType;
use caravel_domain::error::{Error, Result};
use caravel_domain::relays::{Relay, RelayStatus};
use caravel_util::nostr::Key;

/// Insert a relay. A taken subdomain is the caller's to fix.
pub async fn insert_relay(conn: &mut PgConnection, relay: &Relay) -> Result<()> {
    sqlx::query(
        "INSERT INTO relay (
            id, tenant_pubkey, subdomain, domain, plan_id, status, pubkey, secret,
            info_name, info_icon, info_description,
            policy_public_read, policy_public_write, policy_public_join, policy_strip_signatures,
            groups_enabled, management_enabled, members_can_invite, blossom_enabled,
            livekit_enabled, push_enabled, custom_domain, created_at, policy_anonymous_read,
            blossom_authenticated_read, member_price, member_interval
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17,
            $18, $19, $20, $21, $22, $23, $24, $25, $26, $27)",
    )
    .bind(&relay.id)
    .bind(&relay.tenant_pubkey)
    .bind(&relay.subdomain)
    .bind(&relay.domain)
    .bind(&relay.plan_id)
    .bind(relay.status)
    .bind(&relay.pubkey)
    .bind(&relay.secret)
    .bind(&relay.info_name)
    .bind(&relay.info_icon)
    .bind(&relay.info_description)
    .bind(relay.policy_public_read)
    .bind(relay.policy_public_write)
    .bind(relay.policy_public_join)
    .bind(relay.policy_strip_signatures)
    .bind(relay.groups_enabled)
    .bind(relay.management_enabled)
    .bind(relay.members_can_invite)
    .bind(relay.blossom_enabled)
    .bind(relay.livekit_enabled)
    .bind(relay.push_enabled)
    .bind(&relay.custom_domain)
    .bind(relay.created_at)
    .bind(relay.policy_anonymous_read)
    .bind(relay.blossom_authenticated_read)
    .bind(relay.member_price)
    .bind(relay.member_interval)
    .execute(conn)
    .await
    .map_err(|error| map_relay_write_error(error.into()))?;
    Ok(())
}

/// Write a relay's settings. Its redirect hosts, status, keys and purge marker
/// have writers of their own. Verification of its custom domain is kept unless
/// the custom domain or the canonical host it must point at changed, comparing
/// against the stored row so a verification that landed since the relay was read
/// survives.
pub async fn update_relay(conn: &mut PgConnection, relay: &Relay) -> Result<()> {
    sqlx::query(
        "UPDATE relay SET
            subdomain = $2, domain = $3, plan_id = $4,
            info_name = $5, info_icon = $6, info_description = $7,
            policy_public_read = $8, policy_public_write = $9,
            policy_public_join = $10, policy_strip_signatures = $11,
            groups_enabled = $12, management_enabled = $13, members_can_invite = $14,
            blossom_enabled = $15, livekit_enabled = $16, push_enabled = $17,
            custom_domain = $18,
            custom_domain_verified = COALESCE(
                custom_domain_verified AND custom_domain = $18 AND domain = $3 AND subdomain = $2,
                false
            ),
            policy_anonymous_read = $19,
            blossom_authenticated_read = $20, member_price = $21, member_interval = $22
         WHERE id = $1",
    )
    .bind(&relay.id)
    .bind(&relay.subdomain)
    .bind(&relay.domain)
    .bind(&relay.plan_id)
    .bind(&relay.info_name)
    .bind(&relay.info_icon)
    .bind(&relay.info_description)
    .bind(relay.policy_public_read)
    .bind(relay.policy_public_write)
    .bind(relay.policy_public_join)
    .bind(relay.policy_strip_signatures)
    .bind(relay.groups_enabled)
    .bind(relay.management_enabled)
    .bind(relay.members_can_invite)
    .bind(relay.blossom_enabled)
    .bind(relay.livekit_enabled)
    .bind(relay.push_enabled)
    .bind(&relay.custom_domain)
    .bind(relay.policy_anonymous_read)
    .bind(relay.blossom_authenticated_read)
    .bind(relay.member_price)
    .bind(relay.member_interval)
    .execute(conn)
    .await
    .map_err(|error| map_relay_write_error(error.into()))?;
    Ok(())
}

/// Change the name, description or icon the relay publishes, leaving `None`s as
/// they are.
pub async fn set_relay_info(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    name: Option<&str>,
    description: Option<&str>,
    icon: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE relay SET info_name = COALESCE($2, info_name), info_description = COALESCE($3, info_description),
            info_icon = COALESCE($4, info_icon)
         WHERE id = $1",
    )
    .bind(relay_id)
    .bind(name)
    .bind(description)
    .bind(icon)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_relay_status(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    status: RelayStatus,
) -> Result<()> {
    sqlx::query("UPDATE relay SET status = $1 WHERE id = $2")
        .bind(status)
        .bind(relay_id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn set_relay_purged_at(db: impl PgExecutor<'_>, relay_id: &str, at: i64) -> Result<()> {
    sqlx::query("UPDATE relay SET purged_at = $1 WHERE id = $2")
        .bind(at)
        .bind(relay_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Record whether DNS verification sees the relay's custom domain pointing at it,
/// but only if the custom domain is still `custom_domain` and no other live relay
/// has it verified. Returns whether it wrote.
pub async fn set_custom_domain_verified(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    custom_domain: &str,
    verified: bool,
) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE relay SET custom_domain_verified = $1
         WHERE id = $2 AND custom_domain = $3 AND purged_at IS NULL
           AND NOT ($1 AND EXISTS (
               SELECT 1 FROM relay other
               WHERE other.custom_domain = $3 AND other.custom_domain_verified
                 AND other.purged_at IS NULL AND other.id <> $2
           ))",
    )
    .bind(verified)
    .bind(relay_id)
    .bind(custom_domain)
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Make `host` redirect to a relay, taking it from whichever relay it redirected to.
pub async fn add_host_redirect(
    db: impl PgExecutor<'_>,
    host: &str,
    relay_id: &str,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO host_redirect (host, relay_id, created_at) VALUES ($1, $2, $3)
         ON CONFLICT (host) DO UPDATE SET relay_id = EXCLUDED.relay_id, created_at = EXCLUDED.created_at",
    )
    .bind(host)
    .bind(relay_id)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

/// Stop redirecting to a relay the hosts it now answers on itself. Another
/// relay's redirects stay, since naming a domain doesn't prove holding it.
pub async fn remove_host_redirects(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    hosts: &[String],
) -> Result<()> {
    sqlx::query("DELETE FROM host_redirect WHERE relay_id = $1 AND host = ANY($2)")
        .bind(relay_id)
        .bind(hosts)
        .execute(db)
        .await?;
    Ok(())
}

/// Stop redirecting `host`, for a relay that has verified it as its own domain,
/// returning the relay it redirected to.
pub async fn take_host_redirect(db: impl PgExecutor<'_>, host: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("DELETE FROM host_redirect WHERE host = $1 RETURNING relay_id")
            .bind(host)
            .fetch_optional(db)
            .await?,
    )
}

/// Record that a purged relay's data was erased.
pub async fn set_relay_erased_at(db: impl PgExecutor<'_>, id: &str, now: i64) -> Result<()> {
    sqlx::query("UPDATE relay SET erased_at = $2 WHERE id = $1")
        .bind(id)
        .bind(now)
        .execute(db)
        .await?;
    Ok(())
}

/// Mark a relay purged and record it, keeping the status it was purged from on the
/// activity so billing knows whether to credit it.
pub async fn purge_relay_in_tx(
    conn: &mut sqlx::PgConnection,
    relay: &Relay,
    now: i64,
) -> Result<()> {
    commands::relays::set_relay_purged_at(&mut *conn, &relay.id, now).await?;
    commands::activity::insert_relay_activity(
        conn,
        ActivityType::PurgeRelay,
        relay,
        relay.status,
        now,
    )
    .await?;
    Ok(())
}

/// A taken subdomain is the caller's to fix.
fn map_relay_write_error(error: Error) -> Error {
    let constraint = match &error {
        Error::Internal(inner) => inner
            .downcast_ref::<sqlx::Error>()
            .and_then(caravel_util::db::unique_violation)
            .map(str::to_string),
        _ => None,
    };

    match constraint.as_deref() {
        Some("relay_subdomain_key") => {
            Error::invalid("subdomain-exists", "subdomain already exists")
        }
        _ => error,
    }
}

/// Rewrite every event the relay publishes about its own state from the tables
/// that hold it: the member list, each role, and each room's metadata, admins,
/// members and pins.
pub async fn rebuild_derived(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    now: i64,
) -> Result<Vec<Event>> {
    let mut published =
        vec![commands::memberships::publish_member_list(&mut *conn, key, relay_id, now).await?];

    for role in queries::roles::list_roles(&mut *conn, relay_id).await? {
        published.push(commands::roles::publish_role(&mut *conn, key, relay_id, &role, now).await?);
    }

    for room in queries::rooms::list_rooms(&mut *conn, relay_id).await? {
        published.push(
            commands::rooms::publish_metadata(&mut *conn, key, relay_id, &room.id, now).await?,
        );
        published
            .push(commands::rooms::publish_admins(&mut *conn, key, relay_id, &room.id, now).await?);
        published.push(
            commands::rooms::publish_members(&mut *conn, key, relay_id, &room.id, now).await?,
        );
        published
            .push(commands::rooms::publish_pins(&mut *conn, key, relay_id, &room.id, now).await?);
    }

    Ok(published)
}

/// Erase what a relay holds, keeping its settings: its events, media, rooms, roles,
/// method grants, bans, claims, notifications and members. Its owner stays, as do
/// the members and invites someone has paid for, and their roles are cleared.
/// Returns the blobs whose bytes should go.
pub async fn erase_relay_data(conn: &mut PgConnection, relay_id: &str) -> Result<Vec<String>> {
    for table in [
        "event",
        "room",
        "role",
        "method_grant",
        "banned_pubkey",
        "banned_event",
        "notification",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE relay_id = $1"))
            .bind(relay_id)
            .execute(&mut *conn)
            .await?;
    }

    sqlx::query(
        "DELETE FROM relay_member WHERE relay_id = $1 AND source <> 'owner'
         AND pubkey NOT IN (
           SELECT pubkey FROM membership
           WHERE relay_id = $1 AND pubkey IS NOT NULL AND status IN ('active', 'canceled')
         )",
    )
    .bind(relay_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query("UPDATE relay_member SET roles = '{}' WHERE relay_id = $1")
        .bind(relay_id)
        .execute(&mut *conn)
        .await?;

    let paid_invites = "SELECT invite_code FROM membership
        WHERE relay_id = $1 AND invite_code IS NOT NULL AND status IN ('active', 'canceled')";
    sqlx::query(&format!(
        "DELETE FROM claim WHERE relay_id = $1 AND code NOT IN ({paid_invites})"
    ))
    .bind(relay_id)
    .execute(&mut *conn)
    .await?;

    Ok(
        sqlx::query_scalar("DELETE FROM blob WHERE relay_id = $1 RETURNING sha256")
            .bind(relay_id)
            .fetch_all(conn)
            .await?,
    )
}

/// Erase everything a purged relay held: its events, access lists, rooms, claims,
/// notifications, media and redirects. Billing's records of it stay.
pub async fn erase_purged_relay_data(conn: &mut PgConnection, relay_id: &str) -> Result<()> {
    for table in [
        "event",
        "room",
        "relay_member",
        "role",
        "banned_pubkey",
        "banned_event",
        "claim",
        "blob",
        "notification",
        "host_redirect",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE relay_id = $1"))
            .bind(relay_id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}
