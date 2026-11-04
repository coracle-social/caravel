//! Events as the event store keeps them: every relay's events in one table, told
//! apart by relay. Replaceable and addressable events occupy one row per slot, and
//! NIP-09 deletions and NIP-62 vanish requests stay stored as tombstones.

use coracle_lib::events::Event;
use coracle_lib::keys::PublicKey;

/// What a relay's policy reads about an event without its content: who wrote it,
/// when, what kind it is, and its single-letter tags as `name:value`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventHead {
    pub id: [u8; 32],
    pub pubkey: PublicKey,
    pub created_at: i64,
    pub kind: u16,
    pub tag_values: Vec<String>,
}

/// Events that share everything a relay's rules read about an event: its kind, its
/// author, and the first values of its `d` and `h` tags, which name its room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventGroup {
    pub kind: u16,
    pub pubkey: PublicKey,
    pub d: Option<String>,
    pub h: Option<String>,
    pub count: u64,
}

/// An event as stored, with its tags as the JSON it was signed with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredEvent {
    pub head: EventHead,
    pub content: String,
    pub tags: String,
    pub sig: [u8; 64],
}

/// A page of a relay's events as the dashboards ask for one, newest first with ties
/// broken by id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventPage {
    /// Only events of these kinds, unless empty.
    pub kinds: Vec<u16>,
    /// Only events with these hex ids, unless empty.
    pub ids: Vec<String>,
    /// Only events created at or before this.
    pub until: Option<i64>,
    /// With `until`, skip the events created at `until` whose id doesn't sort after
    /// this hex id, so a page picks up after the last event of the one before.
    pub after: Option<String>,
    /// Leave out the events the relay signed itself.
    pub exclude_own: bool,
    pub limit: usize,
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
            pubkey: event.pubkey,
            created_at: event.created_at,
            kind: event.kind,
            tag_values: tag_values(event),
        }
    }

    pub fn id_hex(&self) -> String {
        hex::encode(self.id)
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
            self.head.pubkey.to_hex(),
            self.head.created_at,
            self.head.kind,
            self.tags,
            serde_json::to_string(&self.content).expect("strings serialize"),
            sig,
        )
    }

    /// The event, read back from its JSON.
    pub fn to_event(&self) -> serde_json::Result<Event> {
        serde_json::from_str(&self.to_json(false))
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

#[cfg(test)]
mod tests {
    use coracle_lib::events::EventContent;
    use coracle_lib::keys::SecretKey;
    use coracle_lib::tags::Tags;

    use super::*;

    fn event() -> Event {
        let secret = SecretKey::generate();
        let hashed = EventContent::new()
            .with_content("a \"quote\", a \\ backslash and a newline\n")
            .with_tags(
                Tags::new()
                    .add("t", ["with \"quotes\" and \\slashes\\"])
                    .add("e", ["abc"]),
            )
            .with_kind(1)
            .with_created_at(1_700_000_000)
            .with_pubkey(secret.public_key())
            .with_id();
        let sig = secret.sign(hashed.id.as_bytes());
        hashed.with_sig(sig)
    }

    #[test]
    fn an_event_reads_back_from_the_json_it_is_sent_as() {
        let event = event();
        let stored = StoredEvent::from_event(&event);

        assert_eq!(stored.to_event().unwrap(), event);

        let stripped: serde_json::Value = serde_json::from_str(&stored.to_json(true)).unwrap();
        assert!(stripped.get("sig").is_none(), "{stripped}");
    }
}
