//! NIP-42: the challenge each connection is issued, and checking a client's answer.

use coracle_lib::events::Event;

/// The kind a client answers a challenge with.
pub const KIND: u16 = 22242;

/// How far an answer's timestamp may be from the relay's clock.
pub const MAX_DRIFT: i64 = 600;

/// A fresh challenge, unguessable and unique to one connection.
pub fn challenge() -> String {
    hex::encode(rand::random::<[u8; 16]>())
}

/// Check an answer, whose signature is already verified, against the challenge
/// this connection was issued and the host it connected to, returning the pubkey
/// it proves. The relay tag is compared by host, so an answer naming `wss://` or
/// `ws://`, with or without a trailing slash, is the same relay.
pub fn validate(event: &Event, challenge: &str, host: &str, now: i64) -> Result<String, String> {
    if event.kind != KIND {
        return Err(format!("expected kind {KIND}, got {}", event.kind));
    }

    if (now - event.created_at).abs() > MAX_DRIFT {
        return Err("the auth event is too far from the relay's clock".to_string());
    }

    if event.tags.value("challenge") != Some(challenge) {
        return Err("the challenge does not match the one this connection issued".to_string());
    }

    let relay = event
        .tags
        .value("relay")
        .ok_or("the auth event has no relay tag")?;
    if relay_host(relay) != host {
        return Err(format!(
            "the relay tag names {relay}, which is not this relay"
        ));
    }

    Ok(event.pubkey.to_hex())
}

/// A relay url's host, lowercased, without scheme, port, path or trailing dot.
pub fn relay_host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or_default();
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, port)| {
            if port.chars().all(|c| c.is_ascii_digit()) {
                host
            } else {
                authority
            }
        });
    host.trim_end_matches('.').to_lowercase()
}

#[cfg(test)]
mod tests {
    use coracle_lib::events::EventContent;
    use coracle_lib::keys::SecretKey;
    use coracle_lib::tags::Tags;

    use super::*;

    const NOW: i64 = 1_700_000_000;

    fn answer(secret: &SecretKey, tags: Tags, kind: u16, created_at: i64) -> Event {
        let hashed = EventContent::new()
            .with_tags(tags)
            .with_kind(kind)
            .with_created_at(created_at)
            .with_pubkey(secret.public_key())
            .with_id();
        let sig = secret.sign(hashed.id.as_bytes());
        hashed.with_sig(sig)
    }

    fn tags(challenge: &str, relay: &str) -> Tags {
        Tags::new()
            .add("challenge", [challenge])
            .add("relay", [relay])
    }

    #[test]
    fn a_good_answer_names_its_signer() {
        let secret = SecretKey::generate();
        let event = answer(&secret, tags("abc", "wss://Relay.Example/"), KIND, NOW);
        assert_eq!(
            validate(&event, "abc", "relay.example", NOW),
            Ok(secret.public_key().to_hex())
        );
    }

    #[test]
    fn an_answer_for_another_challenge_or_relay_is_refused() {
        let secret = SecretKey::generate();
        let event = answer(&secret, tags("abc", "wss://relay.example"), KIND, NOW);
        assert!(validate(&event, "xyz", "relay.example", NOW).is_err());
        assert!(validate(&event, "abc", "other.example", NOW).is_err());
    }

    #[test]
    fn an_answer_has_to_be_recent_and_of_the_right_kind() {
        let secret = SecretKey::generate();
        let stale = answer(
            &secret,
            tags("abc", "wss://relay.example"),
            KIND,
            NOW - MAX_DRIFT - 1,
        );
        assert!(validate(&stale, "abc", "relay.example", NOW).is_err());
        let wrong = answer(&secret, tags("abc", "wss://relay.example"), 1, NOW);
        assert!(validate(&wrong, "abc", "relay.example", NOW).is_err());
    }

    #[test]
    fn relay_hosts_ignore_scheme_port_and_path() {
        assert_eq!(relay_host("ws://Relay.Example:8080/path"), "relay.example");
        assert_eq!(relay_host("relay.example."), "relay.example");
    }
}
