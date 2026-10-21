//! The event store: every relay's events in one table, told apart by relay.
//! Replaceable and addressable events occupy one row per slot, overwritten by a
//! newer version, and NIP-09 deletions and NIP-62 vanish requests stay stored as
//! tombstones that refuse what they removed.

use coracle_lib::addresses::get_identifier;
use coracle_lib::events::Event;
use coracle_lib::expiration::get_expiration;
use coracle_lib::filters::Filter;
use coracle_lib::kinds::{is_addressable, is_replaceable};
use sqlx::postgres::PgRow;
use sqlx::{PgConnection, PgExecutor, Postgres, QueryBuilder, Row};

use crate::Core;
use crate::crypto::RelayKey;
use crate::error::Result;

pub mod kinds;

use kinds::{DELETION, VANISH};

/// What a relay's policy reads about an event without its content: who wrote it,
/// when, what kind it is, and its single-letter tags as `name:value`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventHead {
    pub id: [u8; 32],
    pub pubkey: [u8; 32],
    pub created_at: i64,
    pub kind: u16,
    pub tag_values: Vec<String>,
}

/// Events that share everything a relay's rules read about an event: its kind, its
/// author, and the first values of its `d` and `h` tags, which name its room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventGroup {
    pub kind: u16,
    pub pubkey: [u8; 32],
    pub d: Option<String>,
    pub h: Option<String>,
    pub count: i64,
}

/// An event as stored, with its tags as the JSON it was signed with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredEvent {
    pub head: EventHead,
    pub content: String,
    pub tags: String,
    pub sig: [u8; 64],
}

/// What saving an event did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Saved {
    /// The event is new here.
    New,
    /// The relay already has this event, or a newer version of its slot.
    Duplicate,
    /// A stored deletion or vanish request refuses the event.
    Deleted,
}

impl EventHead {
    pub fn from_event(event: &Event) -> Self {
        Self {
            id: event.id.to_bytes(),
            pubkey: event.pubkey.as_bytes(),
            created_at: event.created_at,
            kind: event.kind,
            tag_values: tag_values(event),
        }
    }

    pub fn id_hex(&self) -> String {
        hex::encode(self.id)
    }

    pub fn pubkey_hex(&self) -> String {
        hex::encode(self.pubkey)
    }

    /// The first value of a single-letter tag.
    pub fn tag(&self, name: &str) -> Option<&str> {
        self.tag_values.iter().find_map(|value| {
            let (key, value) = value.split_once(':')?;
            (key == name).then_some(value)
        })
    }
}

impl StoredEvent {
    pub fn from_event(event: &Event) -> Self {
        Self {
            head: EventHead::from_event(event),
            content: event.content.clone(),
            tags: serde_json::to_string(&event.tags).expect("tags serialize"),
            sig: event.sig,
        }
    }

    /// The event as NIP-01 JSON, without its `sig` field if `strip_signature`.
    pub fn to_json(&self, strip_signature: bool) -> String {
        let sig = if strip_signature {
            String::new()
        } else {
            format!(r#","sig":"{}""#, hex::encode(self.sig))
        };

        format!(
            r#"{{"id":"{}","pubkey":"{}","created_at":{},"kind":{},"tags":{},"content":{}{}}}"#,
            self.head.id_hex(),
            self.head.pubkey_hex(),
            self.head.created_at,
            self.head.kind,
            self.tags,
            serde_json::to_string(&self.content).expect("strings serialize"),
            sig,
        )
    }
}

/// An event's single-letter tags as `name:value`, the form tag filters match.
fn tag_values(event: &Event) -> Vec<String> {
    event
        .tags
        .iter()
        .filter(|tag| tag.len() >= 2 && tag.name().chars().count() == 1)
        .map(|tag| format!("{}:{}", tag.name(), tag.value()))
        .collect()
}

/// The slot a replaceable or addressable event occupies: `""` for replaceable
/// kinds and the `d` tag for addressable ones. Regular events have none.
pub fn slot(event: &Event) -> Option<String> {
    if is_replaceable(event.kind) {
        Some(String::new())
    } else if is_addressable(event.kind) {
        Some(get_identifier(&event.tags).to_string())
    } else {
        None
    }
}

/// Whether Postgres text can hold the event: no NUL characters.
pub fn is_storable(event: &Event) -> bool {
    !event.content.contains('\0')
        && !event
            .tags
            .iter()
            .any(|tag| tag.as_slice().iter().any(|value| value.contains('\0')))
}

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
        && is_tombstoned(&mut *conn, relay_id, event, slot(event).as_deref()).await?
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

/// A tag from its values.
pub fn tag(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

/// Store an event the relay signed to publish its own state, which no tombstone
/// refuses: the state it describes is the authority.
async fn save_relay_event(
    conn: &mut PgConnection,
    relay_id: &str,
    event: &Event,
    now: i64,
) -> Result<Saved> {
    let slot = slot(event);
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

/// Whether the author deleted this event, or the version of its slot it is, or
/// asked the relay to forget them since it was written.
async fn is_tombstoned(
    conn: &mut PgConnection,
    relay_id: &str,
    event: &Event,
    slot: Option<&str>,
) -> Result<bool> {
    let pubkey = event.pubkey.to_hex();
    let by_id = format!("e:{}", event.id.to_hex());
    let by_address = slot.map(|d| format!("a:{}:{pubkey}:{d}", event.kind));

    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM event
            WHERE relay_id = $1 AND pubkey = $2 AND (
                (kind = 5 AND (tag_values @> ARRAY[$3] OR (tag_values @> ARRAY[$4] AND created_at >= $5)))
                OR (kind = 62 AND created_at >= $5)
            )
        )",
    )
    .bind(relay_id)
    .bind(event.pubkey.as_bytes().to_vec())
    .bind(by_id)
    .bind(by_address.unwrap_or_default())
    .bind(event.created_at)
    .fetch_one(conn)
    .await?)
}

/// Delete the events a NIP-09 deletion names by id or address, where its author
/// wrote them. A named address is deleted up to the deletion's `created_at`.
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
         WHERE relay_id = $1 AND pubkey = $2 AND kind <> 5 AND (
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

/// A relay's unexpired events matching a filter, newest first with ties broken by
/// id, at most `max_limit` of them whatever the filter asks.
pub async fn query_events(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    filter: &Filter,
    max_limit: usize,
    now: i64,
) -> Result<Vec<StoredEvent>> {
    if filter.matches_nothing() || filter.limit == Some(0) {
        return Ok(Vec::new());
    }

    let mut query = QueryBuilder::<Postgres>::new(
        "SELECT id, pubkey, created_at, kind, tag_values, content, tags, sig FROM event",
    );
    push_filter(&mut query, relay_id, filter, now);
    query
        .push(" ORDER BY created_at DESC, id LIMIT ")
        .push_bind(filter.limit.unwrap_or(max_limit).min(max_limit) as i64);

    let rows = query.build().fetch_all(db).await?;
    Ok(rows.iter().filter_map(stored_event).collect())
}

/// The heads of a relay's unexpired events matching a filter, oldest first, or
/// `None` if there are more than `max`.
pub async fn query_event_heads(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    filter: &Filter,
    max: usize,
    now: i64,
) -> Result<Option<Vec<EventHead>>> {
    if filter.matches_nothing() {
        return Ok(Some(Vec::new()));
    }

    let mut query =
        QueryBuilder::<Postgres>::new("SELECT id, pubkey, created_at, kind, tag_values FROM event");
    push_filter(&mut query, relay_id, filter, now);
    query
        .push(" ORDER BY created_at, id LIMIT ")
        .push_bind(max as i64 + 1);

    let rows = query.build().fetch_all(db).await?;
    if rows.len() > max {
        return Ok(None);
    }
    Ok(Some(rows.iter().filter_map(event_head).collect()))
}

/// A relay's unexpired events matching any of `filters`, each counted once, in
/// groups that share what the rules read, so a caller can count only those a
/// reader may see. A filter's limit bounds what is sent, not what is counted.
pub async fn count_event_groups(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    filters: &[Filter],
    now: i64,
) -> Result<Vec<EventGroup>> {
    let filters: Vec<&Filter> = filters
        .iter()
        .filter(|filter| !filter.matches_nothing())
        .collect();
    if filters.is_empty() {
        return Ok(Vec::new());
    }

    let first_value = |name: char| {
        format!(
            "(SELECT substr(value, 3) FROM unnest(tag_values) WITH ORDINALITY AS tag(value, position)
              WHERE value LIKE '{name}:%' ORDER BY position LIMIT 1)"
        )
    };
    let mut query = QueryBuilder::<Postgres>::new(format!(
        "SELECT kind, pubkey, {} AS d_value, {} AS h_value, count(*) AS count FROM event",
        first_value('d'),
        first_value('h'),
    ));
    push_scope(&mut query, relay_id, now);
    query.push(" AND (");
    for (index, filter) in filters.iter().enumerate() {
        if index > 0 {
            query.push(" OR ");
        }
        query.push("(true");
        push_conditions(&mut query, filter);
        query.push(")");
    }
    query.push(") GROUP BY 1, 2, 3, 4");

    let rows = query.build().fetch_all(db).await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            Some(EventGroup {
                kind: u16::try_from(row.get::<i32, _>("kind")).ok()?,
                pubkey: row.get::<Vec<u8>, _>("pubkey").try_into().ok()?,
                d: row.get("d_value"),
                h: row.get("h_value"),
                count: row.get("count"),
            })
        })
        .collect())
}

/// A page of a relay's events, newest first with ties broken by id, starting
/// after the event at `created_at` with id `id`.
pub async fn list_events_after(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    created_at: i64,
    id: &[u8],
    limit: i64,
) -> Result<Vec<StoredEvent>> {
    let rows = sqlx::query(
        "SELECT id, pubkey, created_at, kind, tag_values, content, tags, sig FROM event
         WHERE relay_id = $1 AND (created_at < $2 OR (created_at = $2 AND id > $3))
         ORDER BY created_at DESC, id LIMIT $4",
    )
    .bind(relay_id)
    .bind(created_at)
    .bind(id)
    .bind(limit)
    .fetch_all(db)
    .await?;

    Ok(rows.iter().filter_map(stored_event).collect())
}

/// Events of one relay by id, in no order.
pub async fn get_events(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    ids: &[[u8; 32]],
) -> Result<Vec<StoredEvent>> {
    let rows = sqlx::query(
        "SELECT id, pubkey, created_at, kind, tag_values, content, tags, sig FROM event
         WHERE relay_id = $1 AND id = ANY($2)",
    )
    .bind(relay_id)
    .bind(ids.iter().map(|id| id.to_vec()).collect::<Vec<_>>())
    .fetch_all(db)
    .await?;

    Ok(rows.iter().filter_map(stored_event).collect())
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

/// Delete everything a pubkey wrote on a relay.
pub async fn delete_events_by_author(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    pubkey: &[u8; 32],
) -> Result<()> {
    sqlx::query("DELETE FROM event WHERE relay_id = $1 AND pubkey = $2")
        .bind(relay_id)
        .bind(pubkey.to_vec())
        .execute(db)
        .await?;
    Ok(())
}

/// Delete every relay's events whose NIP-40 expiration passed, a batch at a time
/// so other writers get between them.
pub async fn sweep_expired_events(core: &Core) -> Result<()> {
    const BATCH: i64 = 1000;

    loop {
        let removed = delete_expired_events(core.db.pool(), core.clock.now(), BATCH).await?;
        if removed > 0 {
            tracing::info!(removed, "swept expired events");
        }
        if removed < BATCH as u64 {
            return Ok(());
        }
    }
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

/// Constrain a query to one relay's unexpired events matching a filter.
fn push_filter(query: &mut QueryBuilder<'_, Postgres>, relay_id: &str, filter: &Filter, now: i64) {
    push_scope(query, relay_id, now);
    push_conditions(query, filter);
}

/// Constrain a query to one relay's unexpired events.
fn push_scope(query: &mut QueryBuilder<'_, Postgres>, relay_id: &str, now: i64) {
    query
        .push(" WHERE relay_id = ")
        .push_bind(relay_id.to_string())
        .push(" AND (expires_at IS NULL OR expires_at > ")
        .push_bind(now)
        .push(")");
}

/// A filter's conditions, each led by ` AND`. A tag constraint on a multi-letter
/// name, or with no values, matches nothing, since only single-letter tags are
/// indexed.
fn push_conditions(query: &mut QueryBuilder<'_, Postgres>, filter: &Filter) {
    if let Some(ids) = &filter.ids {
        let ids: Vec<Vec<u8>> = ids.iter().map(|id| id.as_bytes().to_vec()).collect();
        query.push(" AND id = ANY(").push_bind(ids).push(")");
    }

    if let Some(authors) = &filter.authors {
        let authors: Vec<Vec<u8>> = authors
            .iter()
            .map(|author| author.as_bytes().to_vec())
            .collect();
        query
            .push(" AND pubkey = ANY(")
            .push_bind(authors)
            .push(")");
    }

    if let Some(kinds) = &filter.kinds {
        let kinds: Vec<i32> = kinds.iter().copied().map(i32::from).collect();
        query.push(" AND kind = ANY(").push_bind(kinds).push(")");
    }

    for (key, values) in &filter.tags {
        let name = &key[1..];
        if name.chars().count() != 1 || values.is_empty() {
            query.push(" AND false");
            continue;
        }

        let wanted: Vec<String> = values
            .iter()
            .map(|value| format!("{name}:{value}"))
            .collect();
        let operator = if key.starts_with('&') { " @> " } else { " && " };
        query
            .push(" AND tag_values")
            .push(operator)
            .push_bind(wanted);
    }

    if let Some(since) = filter.since {
        query.push(" AND created_at >= ").push_bind(since);
    }

    if let Some(until) = filter.until {
        query.push(" AND created_at <= ").push_bind(until);
    }

    // phraseto_tsquery reads plain words, so a search can't inject tsquery syntax.
    if let Some(search) = &filter.search {
        query
            .push(" AND search @@ phraseto_tsquery('simple', ")
            .push_bind(search.clone())
            .push(")");
    }
}

fn event_head(row: &PgRow) -> Option<EventHead> {
    Some(EventHead {
        id: row.get::<Vec<u8>, _>("id").try_into().ok()?,
        pubkey: row.get::<Vec<u8>, _>("pubkey").try_into().ok()?,
        created_at: row.get("created_at"),
        kind: u16::try_from(row.get::<i32, _>("kind")).ok()?,
        tag_values: row.get("tag_values"),
    })
}

/// An event from a row holding the event table's columns.
pub(crate) fn stored_event(row: &PgRow) -> Option<StoredEvent> {
    Some(StoredEvent {
        head: event_head(row)?,
        content: row.get("content"),
        tags: row.get("tags"),
        sig: row.get::<Vec<u8>, _>("sig").try_into().ok()?,
    })
}
