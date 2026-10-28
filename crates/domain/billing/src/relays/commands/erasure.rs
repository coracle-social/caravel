//! Erasing what a purged relay held, and rewriting the events it publishes
//! about its own state.

use coracle_lib::events::Event;
use sqlx::{PgConnection, PgExecutor};

use caravel_util::error::Result;
use caravel_util::nostr::Key;

use crate::relays::{ActivityType, Relay};

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
    super::rows::set_relay_purged_at(&mut *conn, &relay.id, now).await?;
    super::activity::insert_relay_activity(
        conn,
        ActivityType::PurgeRelay,
        relay,
        relay.status,
        now,
    )
    .await?;
    Ok(())
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
    let mut published = vec![
        caravel_relays::access::commands::publish_member_list(&mut *conn, key, relay_id, now)
            .await?,
    ];

    for role in caravel_relays::access::queries::list_roles(&mut *conn, relay_id).await? {
        published.push(
            caravel_relays::access::commands::publish_role(&mut *conn, key, relay_id, &role, now)
                .await?,
        );
    }

    for room in caravel_relays::rooms::queries::list_rooms(&mut *conn, relay_id).await? {
        published.push(
            caravel_relays::rooms::commands::publish_metadata(
                &mut *conn, key, relay_id, &room.id, now,
            )
            .await?,
        );
        published.push(
            caravel_relays::rooms::commands::publish_admins(
                &mut *conn, key, relay_id, &room.id, now,
            )
            .await?,
        );
        published.push(
            caravel_relays::rooms::commands::publish_members(
                &mut *conn, key, relay_id, &room.id, now,
            )
            .await?,
        );
        published.push(
            caravel_relays::rooms::commands::publish_pins(&mut *conn, key, relay_id, &room.id, now)
                .await?,
        );
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
