//! Banning pubkeys and events from a relay, and lifting bans.

use coracle_lib::events::Event;
use sqlx::{PgConnection, PgExecutor};

use crate::commands;
use caravel_domain as domain;
use caravel_domain::error::{Error, Result};
use caravel_util::nostr::Key;

/// Ban a pubkey: remove it as a member, record the ban, and delete everything it
/// wrote on the relay but its tombstones, so what it deleted stays deleted.
pub async fn ban_pubkey(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    tenant_pubkey: &str,
    pubkey: &str,
    reason: &str,
    now: i64,
) -> Result<Vec<Event>> {
    let published =
        commands::memberships::remove(&mut *conn, key, relay_id, tenant_pubkey, pubkey, now)
            .await?;

    sqlx::query(
        "INSERT INTO banned_pubkey (relay_id, pubkey, reason, created_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT (relay_id, pubkey) DO NOTHING",
    )
    .bind(relay_id)
    .bind(pubkey)
    .bind(reason)
    .bind(now)
    .execute(&mut *conn)
    .await?;

    let author = domain::events::decode_id(pubkey)
        .ok_or_else(|| Error::invalid("invalid-pubkey", "invalid pubkey"))?;
    commands::events::delete_events_by_author(&mut *conn, relay_id, &author).await?;

    Ok(published)
}

pub async fn unban_pubkey(db: impl PgExecutor<'_>, relay_id: &str, pubkey: &str) -> Result<()> {
    sqlx::query("DELETE FROM banned_pubkey WHERE relay_id = $1 AND pubkey = $2")
        .bind(relay_id)
        .bind(pubkey)
        .execute(db)
        .await?;
    Ok(())
}

/// Ban an event: delete it and refuse it from then on.
pub async fn ban_event(
    conn: &mut PgConnection,
    relay_id: &str,
    event_id: &str,
    reason: &str,
    now: i64,
) -> Result<()> {
    let id = domain::events::decode_id(event_id)
        .ok_or_else(|| Error::invalid("invalid-id", "invalid event id"))?;
    commands::events::delete_event(&mut *conn, relay_id, &id).await?;

    sqlx::query(
        "INSERT INTO banned_event (relay_id, event_id, reason, created_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT (relay_id, event_id) DO UPDATE SET reason = EXCLUDED.reason",
    )
    .bind(relay_id)
    .bind(event_id)
    .bind(reason)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn allow_event(db: impl PgExecutor<'_>, relay_id: &str, event_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM banned_event WHERE relay_id = $1 AND event_id = $2")
        .bind(relay_id)
        .bind(event_id)
        .execute(db)
        .await?;
    Ok(())
}
