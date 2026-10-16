//! Calls: the LiveKit rooms a relay's NIP-29 rooms meet in, and the meter of the
//! time participants spend in them, which billing reads.

use coracle_lib::events::Event;
use sqlx::PgExecutor;

use crate::Core;
use crate::crypto::RelayKey;
use crate::error::{Error, Result};
use crate::events;
use crate::rooms::kinds::LIVEKIT_PARTICIPANTS;

/// The LiveKit room a relay's room meets in. Relays share one LiveKit project, so
/// the name leads with the relay.
pub fn livekit_room(relay_id: &str, room_id: &str) -> String {
    format!("{relay_id}:{room_id}")
}

/// The relay and room a LiveKit room belongs to. Relay ids never hold a colon.
pub fn split_livekit_room(name: &str) -> Option<(&str, &str)> {
    name.split_once(':')
}

/// Credit one participant's closed session. A redelivered webhook for the same
/// session changes nothing.
pub async fn record_session(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    sid: &str,
    room_id: &str,
    seconds: i64,
    ended_at: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO livekit_session (relay_id, sid, room_id, seconds, ended_at) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (relay_id, sid) DO NOTHING",
    )
    .bind(relay_id)
    .bind(sid)
    .bind(room_id)
    .bind(seconds)
    .bind(ended_at)
    .execute(db)
    .await?;
    Ok(())
}

/// A relay's lifetime call clock: every participant's time in every room. It only
/// climbs, so billing a period takes how far it moved.
pub async fn participant_seconds(db: impl PgExecutor<'_>, relay_id: &str) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(seconds), 0)::bigint FROM livekit_session WHERE relay_id = $1",
    )
    .bind(relay_id)
    .fetch_one(db)
    .await?)
}

/// Publish who is in a room's call, as the relay. The list is LiveKit's, so it is
/// kept only as this event.
pub async fn publish_participants(
    core: &Core,
    key: &RelayKey,
    relay_id: &str,
    room_id: &str,
    participants: &[String],
) -> Result<Event> {
    let mut tags = vec![vec!["d".to_string(), room_id.to_string()]];
    tags.extend(
        participants
            .iter()
            .map(|pubkey| vec!["participant".to_string(), pubkey.clone()]),
    );

    let now = core.clock.now();
    let event = key.sign(LIVEKIT_PARTICIPANTS, tags, "", now);
    core.db
        .with_tx(async |conn| events::save_relay_event(conn, relay_id, &event, now).await)
        .await?;
    Ok::<_, Error>(event)
}
