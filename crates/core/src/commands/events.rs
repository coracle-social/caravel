//! Writing the event store: keeping events, publishing the relay's own, and acting
//! on deletions and requests to vanish.

use coracle_lib::events::Event;
use coracle_lib::expiration::get_expiration;
use sqlx::{PgConnection, PgExecutor, Postgres, QueryBuilder};

use crate::crypto::RelayKey;
use crate::domain;
use crate::error::Result;
use crate::models::events::{Saved, StoredEvent};
use crate::models::kinds::{DELETION, VANISH};
use crate::queries;

/// Store an event unless a tombstone refuses it, overwriting an older version of
/// its slot. A replaceable event as old as the stored version replaces it, so the
/// last of several in one second wins.
pub async fn save_event(
    conn: &mut PgConnection,
    relay_id: &str,
    event: &Event,
    now: i64,
) -> Result<Saved> {
    if event.kind != DELETION
        && event.kind != VANISH
        && queries::events::is_tombstoned(
            &mut *conn,
            relay_id,
            event,
            domain::events::slot(event).as_deref(),
        )
        .await?
    {
        return Ok(Saved::Deleted);
    }

    save_relay_event(conn, relay_id, event, now).await
}

/// Sign an event as the relay and store it, for the caller to deliver once its
/// transaction commits.
pub async fn publish(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    kind: u16,
    tags: Vec<Vec<String>>,
    now: i64,
) -> Result<Event> {
    let event = key.sign(kind, tags, "", now);
    save_relay_event(conn, relay_id, &event, now).await?;
    Ok(event)
}

/// Store an event the relay signed to publish its own state, which no tombstone
/// refuses: the state it describes is the authority.
async fn save_relay_event(
    conn: &mut PgConnection,
    relay_id: &str,
    event: &Event,
    now: i64,
) -> Result<Saved> {
    let slot = domain::events::slot(event);
    let stored = StoredEvent::from_event(event);
    let mut query = QueryBuilder::<Postgres>::new(
        "INSERT INTO event (relay_id, id, created_at, kind, pubkey, content, tags, sig, tag_values, d, expires_at, seen_at) ",
    );
    query.push_values([&stored], |mut row, stored| {
        row.push_bind(relay_id)
            .push_bind(stored.head.id.to_vec())
            .push_bind(stored.head.created_at)
            .push_bind(i32::from(stored.head.kind))
            .push_bind(stored.head.pubkey.to_vec())
            .push_bind(&stored.content)
            .push_bind(&stored.tags)
            .push_bind(stored.sig.to_vec())
            .push_bind(&stored.head.tag_values)
            .push_bind(&slot)
            .push_bind(get_expiration(&event.tags))
            .push_bind(now);
    });

    if slot.is_some() {
        query.push(
            " ON CONFLICT (relay_id, kind, pubkey, d) WHERE d IS NOT NULL DO UPDATE SET
                id = EXCLUDED.id, seq = DEFAULT, created_at = EXCLUDED.created_at,
                content = EXCLUDED.content, tags = EXCLUDED.tags, sig = EXCLUDED.sig,
                tag_values = EXCLUDED.tag_values, expires_at = EXCLUDED.expires_at, seen_at = EXCLUDED.seen_at
              WHERE event.created_at <= EXCLUDED.created_at AND event.id <> EXCLUDED.id",
        );
    } else {
        query.push(" ON CONFLICT (relay_id, id) DO NOTHING");
    }
    query.push(" RETURNING seq");

    let inserted = query.build().fetch_optional(conn).await?;
    Ok(if inserted.is_some() {
        Saved::New
    } else {
        Saved::Duplicate
    })
}

/// Delete the events a NIP-09 deletion names by id or address, where its author
/// wrote them. A named address is deleted up to the deletion's `created_at`, and
/// deletions and requests to vanish can't be deleted (NIP-09, NIP-62).
pub async fn apply_deletion(
    conn: &mut PgConnection,
    relay_id: &str,
    deletion: &Event,
) -> Result<u64> {
    let author = deletion.pubkey.to_hex();
    let ids: Vec<Vec<u8>> = deletion
        .tags
        .find_all("e")
        .filter_map(|tag| hex::decode(tag.value()).ok().filter(|id| id.len() == 32))
        .collect();
    let addresses: Vec<String> = deletion
        .tags
        .find_all("a")
        .map(|tag| tag.value().to_string())
        .filter(|address| address.split(':').nth(1) == Some(author.as_str()))
        .collect();

    let result = sqlx::query(
        "DELETE FROM event
         WHERE relay_id = $1 AND pubkey = $2 AND kind NOT IN (5, 62) AND (
            id = ANY($3)
            OR (d IS NOT NULL AND created_at <= $5
                AND kind::text || ':' || encode(pubkey, 'hex') || ':' || d = ANY($4))
         )",
    )
    .bind(relay_id)
    .bind(deletion.pubkey.as_bytes().to_vec())
    .bind(ids)
    .bind(addresses)
    .bind(deletion.created_at)
    .execute(conn)
    .await?;

    Ok(result.rows_affected())
}

/// Delete everything a pubkey wrote before `until`, and the gift wraps addressed
/// to it, for a NIP-62 request to vanish.
pub async fn apply_vanish(
    conn: &mut PgConnection,
    relay_id: &str,
    pubkey: &[u8; 32],
    until: i64,
) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM event
         WHERE relay_id = $1 AND created_at <= $3 AND kind <> 62
           AND (pubkey = $2 OR (kind = 1059 AND tag_values @> ARRAY['p:' || encode($2, 'hex')]))",
    )
    .bind(relay_id)
    .bind(pubkey.to_vec())
    .bind(until)
    .execute(conn)
    .await?;

    Ok(result.rows_affected())
}

/// Delete one of a relay's events.
pub async fn delete_event(db: impl PgExecutor<'_>, relay_id: &str, id: &[u8; 32]) -> Result<()> {
    sqlx::query("DELETE FROM event WHERE relay_id = $1 AND id = $2")
        .bind(relay_id)
        .bind(id.to_vec())
        .execute(db)
        .await?;
    Ok(())
}

/// Delete everything a pubkey wrote on a relay but its deletions and requests to
/// vanish, which go on refusing what they removed.
pub async fn delete_events_by_author(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    pubkey: &[u8; 32],
) -> Result<()> {
    sqlx::query("DELETE FROM event WHERE relay_id = $1 AND pubkey = $2 AND kind NOT IN (5, 62)")
        .bind(relay_id)
        .bind(pubkey.to_vec())
        .execute(db)
        .await?;
    Ok(())
}

/// Delete up to `batch` events whose NIP-40 expiration has passed, across every
/// relay, returning how many went.
pub async fn delete_expired_events(db: impl PgExecutor<'_>, now: i64, batch: i64) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM event WHERE ctid IN (
            SELECT ctid FROM event WHERE expires_at IS NOT NULL AND expires_at <= $1 LIMIT $2
        )",
    )
    .bind(now)
    .bind(batch)
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}
