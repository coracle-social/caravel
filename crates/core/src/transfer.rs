//! Moving a relay's events in and out as JSONL, one event per line. An export
//! streams every stored event; an import stores each valid line as it arrives,
//! then rebuilds the rooms the dump describes from their own history.

use std::collections::BTreeMap;

use bytes::Bytes;
use coracle_lib::events::Event;
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::Core;
use crate::bus::Change;
use crate::error::{Error, Result};
use crate::events::{self, Saved};
use crate::management;
use crate::relay_policy;
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

/// Something in an import that didn't become a stored event: a line, the event on
/// it, or a room.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportProblem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
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

    /// Count what another part of the import did.
    fn add(&mut self, other: ImportResult) {
        self.imported += other.imported;
        self.invalid += other.invalid;
        self.skipped += other.skipped;
        self.rooms_rebuilt += other.rooms_rebuilt;
        for problem in other.problems {
            self.note(problem);
        }
    }
}

/// What one line of a dump holds.
enum Line {
    Blank,
    Invalid(ImportProblem),
    /// An event describing the source relay's own state.
    SourceState,
    Event(Event),
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

            let page =
                events::list_events_after(&pool, &relay_id, created_at, &id, EXPORT_PAGE).await?;
            let text: String = page
                .iter()
                .map(|stored| format!("{}\n", stored.to_json(false)))
                .collect();

            let next = page
                .last()
                .filter(|_| page.len() as i64 == EXPORT_PAGE)
                .map(|last| (last.head.created_at, last.head.id.to_vec()));

            if page.is_empty() {
                return Ok(None);
            }
            Ok(Some((Bytes::from(text), next)))
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
    let mut ended = false;

    while !ended {
        match body.next().await {
            Some(chunk) => buffer.extend_from_slice(&chunk.map_err(|error| {
                Error::invalid("unreadable-body", format!("the upload broke off: {error}"))
            })?),
            // The last line needn't end with a newline.
            None => {
                ended = true;
                if !buffer.is_empty() {
                    buffer.push(b'\n');
                }
            }
        }

        while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            let text: Vec<u8> = buffer.drain(..=end).collect();
            line += 1;
            if overlong {
                overlong = false;
                continue;
            }

            match parse_line(line, &text[..text.len() - 1]) {
                Line::Blank => {}
                Line::Invalid(problem) => result.fail(problem),
                Line::SourceState => result.skipped += 1,
                Line::Event(event) => {
                    if relay.groups_enabled
                        && let Some(room_id) = rooms::room_id(&event)
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
            }

            if batch.len() >= IMPORT_BATCH {
                result.add(store_batch(core, relay, std::mem::take(&mut batch)).await?);
            }
        }

        if buffer.len() > MAX_LINE_BYTES {
            if !overlong {
                result.fail(ImportProblem {
                    line: Some(line + 1),
                    event_id: None,
                    message: format!("the line is longer than {MAX_LINE_BYTES} bytes"),
                });
            }
            buffer.clear();
            overlong = true;
        }
    }

    result.add(store_batch(core, relay, batch).await?);
    result.add(rebuild_rooms(core, relay, rooms_touched).await?);
    Ok(result)
}

fn parse_line(line: usize, text: &[u8]) -> Line {
    let text = String::from_utf8_lossy(text);
    let text = text.trim();
    if text.is_empty() {
        return Line::Blank;
    }

    let event: Event = match serde_json::from_str(text) {
        Ok(event) => event,
        Err(error) => {
            return Line::Invalid(ImportProblem {
                line: Some(line),
                event_id: None,
                message: format!("json parse error: {error}"),
            });
        }
    };

    if event.verify().is_err() {
        return Line::Invalid(ImportProblem {
            line: Some(line),
            event_id: Some(event.id.to_hex()),
            message: "invalid signature".to_string(),
        });
    }

    if is_source_state(event.kind) {
        return Line::SourceState;
    }

    Line::Event(event)
}

/// Events the source relay published about its own members, roles, media and
/// rooms, which this relay derives from its own tables instead.
fn is_source_state(kind: u16) -> bool {
    relay_policy::is_relay_written(kind) || rooms::is_metadata_kind(kind)
}

/// Store a batch of events, each with the line it came from.
async fn store_batch(
    core: &Core,
    relay: &Relay,
    batch: Vec<(usize, Event)>,
) -> Result<ImportResult> {
    let mut result = ImportResult::default();
    if batch.is_empty() {
        return Ok(result);
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
                line: Some(*line),
                event_id: Some(event.id.to_hex()),
                message: "a deletion stored on this relay refuses it".to_string(),
            }),
            None => result.fail(ImportProblem {
                line: Some(*line),
                event_id: Some(event.id.to_hex()),
                message: "the event contains a NUL character".to_string(),
            }),
        }
    }
    Ok(result)
}

/// Rebuild each room the dump changed from its own history, then rewrite the
/// relay's published state and tell its subscribers. A room the relay doesn't hold
/// is created only by a create event the dump carries; and a deletion of the room
/// is replayed only when nothing follows it, since anything after it belongs to a
/// room created since.
async fn rebuild_rooms(
    core: &Core,
    relay: &Relay,
    rooms_touched: BTreeMap<String, Vec<Event>>,
) -> Result<ImportResult> {
    let mut result = ImportResult::default();
    if rooms_touched.is_empty() {
        return Ok(result);
    }

    let key = core.relay_key(relay)?;
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
                line: None,
                event_id: None,
                message: format!(
                    "failed to rebuild group {room_id}: the dump carries no create event for it"
                ),
            });
            continue;
        }
        if history.is_empty() {
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

    if result.rooms_rebuilt == 0 {
        return Ok(result);
    }

    let published = core
        .db
        .with_tx(async |conn| management::rebuild_derived(conn, &key, &relay.id, now).await)
        .await?;
    core.bus.publish(Change::AccessChanged {
        relay_id: relay.id.clone(),
        published,
    });
    Ok(result)
}
