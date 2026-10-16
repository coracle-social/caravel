//! Moving a relay's events in and out as JSONL, one event per line. An export
//! streams every stored event; an import stores each valid line as it arrives,
//! then rebuilds the rooms the dump describes from their own history.

use std::collections::BTreeMap;

use bytes::Bytes;
use coracle_lib::events::Event;
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use crate::Core;
use crate::bus::Change;
use crate::crypto::RelayKey;
use crate::error::{Error, Result};
use crate::events::{self, EventHead, Saved, StoredEvent};
use crate::management;
use crate::relay_policy::kinds;
use crate::relays::Relay;
use crate::rooms;

/// The longest line an import takes.
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// How many problems an import reports one by one; past this they are only counted.
const MAX_PROBLEMS: usize = 100;

/// How many events an export reads at a time.
const EXPORT_PAGE: i64 = 1000;

/// How many events an import stores per transaction.
const IMPORT_BATCH: usize = 500;

/// One line of an import that didn't become a stored event.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportProblem {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub line: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub event_id: String,
    pub message: String,
}

/// What an import did. An event the relay already held counts as imported, so
/// importing a dump twice reports the same numbers. Skipped counts the events
/// describing the source relay's own state, which this relay writes itself.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ImportResult {
    pub imported: usize,
    pub invalid: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub skipped: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rooms_rebuilt: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<ImportProblem>,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

impl ImportResult {
    fn fail(&mut self, problem: ImportProblem) {
        self.invalid += 1;
        self.note(problem);
    }

    fn note(&mut self, problem: ImportProblem) {
        if self.problems.len() < MAX_PROBLEMS {
            self.problems.push(problem);
        }
    }
}

/// Every event a relay stores, as JSONL, read a page at a time so a relay of any
/// size streams in bounded memory.
pub fn export_events(
    pool: PgPool,
    relay_id: String,
) -> impl Stream<Item = Result<Bytes>> + Send + 'static {
    futures_util::stream::try_unfold(Some((i64::MAX, Vec::new())), move |cursor| {
        let pool = pool.clone();
        let relay_id = relay_id.clone();
        async move {
            let Some((created_at, id)) = cursor else {
                return Ok(None);
            };

            let rows = sqlx::query(
                "SELECT id, pubkey, created_at, kind, tag_values, content, tags, sig FROM event
                 WHERE relay_id = $1 AND (created_at < $2 OR (created_at = $2 AND id > $3))
                 ORDER BY created_at DESC, id LIMIT $4",
            )
            .bind(&relay_id)
            .bind(created_at)
            .bind(&id)
            .bind(EXPORT_PAGE)
            .fetch_all(&pool)
            .await?;

            let mut page = String::new();
            for row in &rows {
                let stored = StoredEvent {
                    head: EventHead {
                        id: row.get::<Vec<u8>, _>("id").try_into().unwrap_or_default(),
                        pubkey: row
                            .get::<Vec<u8>, _>("pubkey")
                            .try_into()
                            .unwrap_or_default(),
                        created_at: row.get("created_at"),
                        kind: u16::try_from(row.get::<i32, _>("kind")).unwrap_or_default(),
                        tag_values: row.get("tag_values"),
                    },
                    content: row.get("content"),
                    tags: row.get("tags"),
                    sig: row.get::<Vec<u8>, _>("sig").try_into().unwrap_or([0; 64]),
                };
                page.push_str(&stored.to_json(false));
                page.push('\n');
            }

            let next = rows
                .last()
                .filter(|_| rows.len() as i64 == EXPORT_PAGE)
                .map(|row| (row.get::<i64, _>("created_at"), row.get::<Vec<u8>, _>("id")));

            if rows.is_empty() {
                return Ok(None);
            }
            Ok(Some((Bytes::from(page), next)))
        }
    })
}

/// Store a JSONL dump in a relay. A line that isn't a validly signed event is
/// counted and reported rather than ending the import. Events describing the
/// source relay's own state are skipped, and on a relay with rooms, each room the
/// dump creates or changes is rebuilt from its history, oldest first.
pub async fn import_events<S, E>(core: &Core, relay: &Relay, mut body: S) -> Result<ImportResult>
where
    S: Stream<Item = std::result::Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    let mut result = ImportResult::default();
    let mut rooms_touched: BTreeMap<String, Vec<Event>> = BTreeMap::new();
    let mut batch: Vec<(usize, Event)> = Vec::new();
    let mut buffer: Vec<u8> = Vec::new();
    let mut line = 0;
    let mut overlong = false;
    let rooms_enabled = relay.groups_enabled;

    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|error| {
            Error::invalid("unreadable-body", format!("the upload broke off: {error}"))
        })?;
        buffer.extend_from_slice(&chunk);

        while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            let text: Vec<u8> = buffer.drain(..=end).collect();
            line += 1;
            if overlong {
                overlong = false;
                continue;
            }
            parse_line(
                &mut result,
                &mut rooms_touched,
                &mut batch,
                rooms_enabled,
                line,
                &text[..text.len() - 1],
            );
            if batch.len() >= IMPORT_BATCH {
                store_batch(core, relay, &mut result, std::mem::take(&mut batch)).await?;
            }
        }

        if buffer.len() > MAX_LINE_BYTES {
            if !overlong {
                result.fail(ImportProblem {
                    line: line + 1,
                    event_id: String::new(),
                    message: format!("the line is longer than {MAX_LINE_BYTES} bytes"),
                });
            }
            buffer.clear();
            overlong = true;
        }
    }

    if !buffer.is_empty() && !overlong {
        line += 1;
        parse_line(
            &mut result,
            &mut rooms_touched,
            &mut batch,
            rooms_enabled,
            line,
            &buffer,
        );
    }
    store_batch(core, relay, &mut result, batch).await?;

    rebuild_rooms(core, relay, &mut result, rooms_touched).await?;
    Ok(result)
}

fn parse_line(
    result: &mut ImportResult,
    rooms_touched: &mut BTreeMap<String, Vec<Event>>,
    batch: &mut Vec<(usize, Event)>,
    rooms_enabled: bool,
    line: usize,
    text: &[u8],
) {
    let text = String::from_utf8_lossy(text);
    let text = text.trim();
    if text.is_empty() {
        return;
    }

    let event: Event = match serde_json::from_str(text) {
        Ok(event) => event,
        Err(error) => {
            return result.fail(ImportProblem {
                line,
                event_id: String::new(),
                message: format!("json parse error: {error}"),
            });
        }
    };

    if event.verify().is_err() {
        return result.fail(ImportProblem {
            line,
            event_id: event.id.to_hex(),
            message: "invalid signature".to_string(),
        });
    }

    if is_source_state(event.kind) {
        result.skipped += 1;
        return;
    }

    if let Some(room_id) = rooms::room_id(&event)
        && rooms_enabled
        && rooms::is_room_event(event.kind, Some(room_id))
    {
        let history = rooms_touched.entry(room_id.to_string()).or_default();
        if rooms::is_moderation_kind(event.kind)
            || matches!(
                event.kind,
                rooms::kinds::JOIN_REQUEST | rooms::kinds::LEAVE_REQUEST
            )
        {
            history.push(event.clone());
        }
    }

    batch.push((line, event));
}

/// Events the source relay published about its own members, roles, media and
/// rooms, which this relay derives from its own tables instead.
fn is_source_state(kind: u16) -> bool {
    matches!(
        kind,
        kinds::RELAY_ADD_MEMBER
            | kinds::RELAY_REMOVE_MEMBER
            | kinds::RELAY_MEMBERS
            | kinds::RELAY_ROLE
            | kinds::BLOB_DESCRIPTOR
    ) || rooms::is_metadata_kind(kind)
}

async fn store_batch(
    core: &Core,
    relay: &Relay,
    result: &mut ImportResult,
    batch: Vec<(usize, Event)>,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }

    let now = core.clock.now();
    let saved = core
        .db
        .with_tx(async |conn| {
            let mut saved = Vec::with_capacity(batch.len());
            for (_, event) in &batch {
                if events::is_storable(event) {
                    saved.push(Some(
                        events::save_event(&mut *conn, &relay.id, event, now).await?,
                    ));
                } else {
                    saved.push(None);
                }
            }
            Ok::<_, Error>(saved)
        })
        .await?;

    for ((line, event), saved) in batch.iter().zip(saved) {
        match saved {
            Some(Saved::New | Saved::Duplicate) => result.imported += 1,
            Some(Saved::Deleted) => result.fail(ImportProblem {
                line: *line,
                event_id: event.id.to_hex(),
                message: "a deletion stored on this relay refuses it".to_string(),
            }),
            None => result.fail(ImportProblem {
                line: *line,
                event_id: event.id.to_hex(),
                message: "the event contains a NUL character".to_string(),
            }),
        }
    }
    Ok(())
}

/// Rebuild each room the dump touched from its own history, then rewrite the
/// relay's published state and tell its subscribers. A room the relay doesn't hold
/// is created only by a create event the dump carries; and a deletion of the room
/// is replayed only when nothing follows it, since anything after it belongs to a
/// room created since.
async fn rebuild_rooms(
    core: &Core,
    relay: &Relay,
    result: &mut ImportResult,
    rooms_touched: BTreeMap<String, Vec<Event>>,
) -> Result<()> {
    if rooms_touched.is_empty() {
        return Ok(());
    }

    let key = RelayKey::decrypt(&core.config.platform_key, &relay.secret)?;
    let known: Vec<String> = rooms::list_rooms(core.db.pool(), &relay.id)
        .await?
        .into_iter()
        .map(|room| room.id)
        .collect();
    let now = core.clock.now();

    for (room_id, mut history) in rooms_touched {
        let created = history
            .iter()
            .any(|event| event.kind == rooms::kinds::CREATE_GROUP);
        if !known.contains(&room_id) && !created {
            result.note(ImportProblem {
                line: 0,
                event_id: String::new(),
                message: format!(
                    "failed to rebuild group {room_id}: the dump carries no create event for it"
                ),
            });
            continue;
        }

        history.sort_by_key(|event| event.created_at);
        let last = history.len().saturating_sub(1);
        core.db
            .with_tx(async |conn| {
                for (i, event) in history.iter().enumerate() {
                    if event.kind == rooms::kinds::DELETE_GROUP && i < last {
                        continue;
                    }
                    rooms::apply_event(&mut *conn, &key, &relay.id, event, now).await?;
                }
                Ok::<_, Error>(())
            })
            .await?;
        result.rooms_rebuilt += 1;
    }

    let published = core
        .db
        .with_tx(async |conn| management::rebuild_derived(conn, &key, &relay.id, now).await)
        .await?;
    core.bus.publish(Change::AccessChanged {
        relay_id: relay.id.clone(),
        published,
    });
    Ok(())
}
