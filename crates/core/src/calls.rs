//! Calls: the LiveKit rooms a relay's NIP-29 rooms meet in, who is in each, and the
//! meter of the time participants spend in them, which billing reads.

use coracle_lib::events::Event;

use crate::Core;
use crate::commands;
use crate::crypto::RelayKey;
use crate::error::Result;
use crate::models::rooms::kinds::LIVEKIT_PARTICIPANTS;

/// A participant's LiveKit identity, `<pubkey>:<nonce>`, so one pubkey can be in a
/// call from several devices.
pub fn participant_identity(pubkey: &str) -> String {
    format!("{pubkey}:{}", hex::encode(rand::random::<[u8; 8]>()))
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
    core.db
        .with_tx(async |conn| {
            commands::events::publish(conn, key, relay_id, LIVEKIT_PARTICIPANTS, tags.clone(), now)
                .await
        })
        .await
}

/// Credit a participant's time in a call, from when they joined until they left,
/// unless LiveKit didn't say when they joined.
pub async fn record_departure(
    core: &Core,
    relay_id: &str,
    room_id: &str,
    sid: &str,
    joined_at: i64,
    left_at: i64,
) -> Result<()> {
    commands::calls::record_departure(core.db.pool(), relay_id, room_id, sid, joined_at, left_at)
        .await
}
