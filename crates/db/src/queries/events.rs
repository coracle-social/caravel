//! Reading the event store.

use coracle_lib::events::{Event, EventId};
use coracle_lib::filters::Filter;
use sqlx::postgres::PgRow;
use sqlx::{PgConnection, PgExecutor, Postgres, QueryBuilder, Row};

use caravel_domain as domain;
use caravel_util::error::{Error, Result};
use caravel_domain::events::{EventGroup, EventHead, EventPage, StoredEvent};
use caravel_domain::relays::Relay;
use caravel_util::nostr::kinds;

const SELECT_EVENT: &str =
    "SELECT id, pubkey, created_at, kind, tag_values, content, tags, sig FROM event";

/// Whether the author deleted this event, or the version of its slot it is, or
/// asked the relay to forget them since it was written, or a moderator of its room
/// deleted it there.
pub async fn is_tombstoned(
    conn: &mut PgConnection,
    relay_id: &str,
    event: &Event,
    slot: Option<&str>,
) -> Result<bool> {
    let pubkey = event.pubkey.to_hex();
    let by_id = format!("e:{}", event.id.to_hex());
    let by_address = slot.map(|d| format!("a:{}:{pubkey}:{d}", event.kind));
    // A moderator may delete a room's posts, but not its state.
    let in_room = domain::rooms::room_id(event)
        .filter(|_| {
            !domain::rooms::is_metadata_kind(event.kind)
                && !domain::rooms::is_moderation_kind(event.kind)
        })
        .map(|room_id| format!("h:{room_id}"));

    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM event
            WHERE relay_id = $1 AND pubkey = $2 AND (
                (kind = 5 AND (tag_values @> ARRAY[$3] OR (tag_values @> ARRAY[$4] AND created_at >= $5)))
                OR (kind = 62 AND created_at >= $5)
            )
        ) OR EXISTS (
            SELECT 1 FROM event
            WHERE relay_id = $1 AND kind = $6 AND tag_values @> ARRAY[$3, $7]
        )",
    )
    .bind(relay_id)
    .bind(event.pubkey.as_bytes().to_vec())
    .bind(by_id)
    .bind(by_address.unwrap_or_default())
    .bind(event.created_at)
    .bind(i32::from(kinds::rooms::DELETE_EVENT))
    .bind(in_room.unwrap_or_default())
    .fetch_one(conn)
    .await?)
}

/// A page of a relay's unexpired events for the dashboards.
pub async fn page_events(
    db: impl PgExecutor<'_>,
    relay: &Relay,
    page: &EventPage,
    now: i64,
) -> Result<Vec<StoredEvent>> {
    let mut filter = Filter::new();
    if !page.kinds.is_empty() {
        filter.kinds = Some(page.kinds.iter().copied().collect());
    }
    if !page.ids.is_empty() {
        filter.ids = Some(
            page.ids
                .iter()
                .map(|id| EventId::from_hex(id))
                .collect::<std::result::Result<_, _>>()
                .map_err(|_| Error::invalid("invalid-ids", "ids must be hex event ids"))?,
        );
    }
    filter.until = page.until;
    let after = page
        .after
        .as_deref()
        .map(EventId::from_hex)
        .transpose()
        .map_err(|_| Error::invalid("invalid-after", "after must be a hex event id"))?;

    let mut query = QueryBuilder::<Postgres>::new(SELECT_EVENT);
    push_filter(&mut query, &relay.id, &filter, now);
    if let (Some(until), Some(after)) = (page.until, after) {
        query
            .push(" AND (created_at < ")
            .push_bind(until)
            .push(" OR id > ")
            .push_bind(after.as_bytes().to_vec())
            .push(")");
    }
    if page.exclude_own {
        let own = hex::decode(&relay.pubkey).map_err(|error| Error::Internal(error.into()))?;
        query.push(" AND pubkey <> ").push_bind(own);
    }
    query
        .push(" ORDER BY created_at DESC, id LIMIT ")
        .push_bind(page.limit as i64);

    let rows = query.build().fetch_all(db).await?;
    Ok(rows.iter().filter_map(stored_event).collect())
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

    let mut query = QueryBuilder::<Postgres>::new(SELECT_EVENT);
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
    let rows = sqlx::query(&format!(
        "{SELECT_EVENT} WHERE relay_id = $1 AND (created_at < $2 OR (created_at = $2 AND id > $3))
         ORDER BY created_at DESC, id LIMIT $4",
    ))
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
    let rows = sqlx::query(&format!(
        "{SELECT_EVENT} WHERE relay_id = $1 AND id = ANY($2)",
    ))
    .bind(relay_id)
    .bind(ids.iter().map(|id| id.to_vec()).collect::<Vec<_>>())
    .fetch_all(db)
    .await?;

    Ok(rows.iter().filter_map(stored_event).collect())
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
