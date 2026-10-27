//! Calls: naming the LiveKit rooms a relay's NIP-29 rooms meet in, and reading who
//! is in one.

use crate::crypto::is_hex64;

/// The LiveKit room a relay's room meets in. Relays share one LiveKit project, so
/// the name leads with the relay.
pub fn livekit_room(relay_id: &str, room_id: &str) -> String {
    format!("{relay_id}:{room_id}")
}

/// The relay and room a LiveKit room belongs to. Relay ids never hold a colon.
pub fn split_livekit_room(name: &str) -> Option<(&str, &str)> {
    name.split_once(':')
}

/// The distinct pubkeys behind participant identities.
pub fn participant_pubkeys(identities: &[String]) -> Vec<String> {
    let mut pubkeys: Vec<String> = Vec::new();
    for identity in identities {
        let pubkey = identity
            .split(':')
            .next()
            .unwrap_or_default()
            .to_lowercase();
        if is_hex64(&pubkey) && !pubkeys.contains(&pubkey) {
            pubkeys.push(pubkey);
        }
    }
    pubkeys
}
