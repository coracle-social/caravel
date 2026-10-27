//! The NIP-01 wire messages, on both sides of a connection: [`ClientMessage`]
//! for what arrives and [`RelayMessage`] for what goes back. Every message is a
//! JSON array whose first element names its type.

use std::fmt;

use caravel_core::models::policy::Refusal;
use coracle_lib::events::{Event, EventId};
use coracle_lib::filters::Filter;
use serde::de::{self, IgnoredAny, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

/// The longest subscription id NIP-01 permits.
pub const MAX_SUBSCRIPTION_ID_LEN: usize = 64;

/// A subscription id a client chose, between one and [`MAX_SUBSCRIPTION_ID_LEN`]
/// characters long as NIP-01 requires.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SubscriptionId(String);

impl SubscriptionId {
    /// Validate a string as a subscription id.
    pub fn new(id: impl Into<String>) -> Result<Self, InvalidSubscriptionId> {
        let id = id.into();

        if id.is_empty() || id.chars().count() > MAX_SUBSCRIPTION_ID_LEN {
            return Err(InvalidSubscriptionId(id));
        }

        Ok(Self(id))
    }
}

impl Serialize for SubscriptionId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SubscriptionId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let id = String::deserialize(deserializer)?;

        SubscriptionId::new(id).map_err(de::Error::custom)
    }
}

/// A string that cannot be a subscription id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidSubscriptionId(pub String);

impl fmt::Display for InvalidSubscriptionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "subscription id must be 1-{MAX_SUBSCRIPTION_ID_LEN} characters, got {}",
            self.0.chars().count()
        )
    }
}

impl std::error::Error for InvalidSubscriptionId {}

/// A refusal as the answer to a published event.
pub fn refused_event(event_id: EventId, refusal: &Refusal) -> RelayMessage {
    RelayMessage::Ok {
        event_id,
        accepted: refusal.accepted(),
        message: refusal.message(),
    }
}

/// A refusal as the end of a subscription.
pub fn refused_subscription(subscription_id: SubscriptionId, refusal: &Refusal) -> RelayMessage {
    RelayMessage::Closed {
        subscription_id,
        message: refusal.message(),
    }
}

/// A refusal as the end of a reconciliation.
pub fn refused_reconciliation(subscription_id: SubscriptionId, refusal: &Refusal) -> RelayMessage {
    RelayMessage::NegErr {
        subscription_id,
        reason: refusal.message(),
    }
}

/// An event's json as the store rendered it, sent on without parsing it again.
#[derive(Debug, Clone)]
pub struct RawEvent(Box<RawValue>);

impl RawEvent {
    /// Take json that is already an event.
    pub fn from_json(json: String) -> Result<Self, serde_json::Error> {
        RawValue::from_string(json).map(Self)
    }
}

impl PartialEq for RawEvent {
    fn eq(&self, other: &Self) -> bool {
        self.0.get() == other.0.get()
    }
}

impl Eq for RawEvent {}

impl Serialize for RawEvent {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// Something a client said to the relay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientMessage {
    /// `["EVENT", <event>]` — publish an event.
    Event(Event),
    /// `["REQ", <id>, <filter>...]` — open a subscription.
    ///
    /// The filters are a union: an event matching any one of them
    /// belongs to the subscription.
    Req {
        subscription_id: SubscriptionId,
        filters: Vec<Filter>,
    },
    /// `["CLOSE", <id>]` — close a subscription.
    Close(SubscriptionId),
    /// `["COUNT", <id>, <filter>...]` — count what a subscription would
    /// match, without sending it (NIP-45).
    Count {
        subscription_id: SubscriptionId,
        filters: Vec<Filter>,
    },
    /// `["AUTH", <event>]` — answer an auth challenge (NIP-42).
    Auth(Event),
    /// `["NEG-OPEN", <id>, <filter>, <message>]` — start reconciling a set of
    /// events (NIP-77), with the negentropy message still hex.
    NegOpen {
        subscription_id: SubscriptionId,
        filter: Filter,
        message: String,
    },
    /// `["NEG-MSG", <id>, <message>]` — one round of reconciliation.
    NegMsg {
        subscription_id: SubscriptionId,
        message: String,
    },
    /// `["NEG-CLOSE", <id>]` — release a reconciliation.
    NegClose(SubscriptionId),
}

impl<'de> Deserialize<'de> for ClientMessage {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_seq(ClientMessageVisitor)
    }
}

struct ClientMessageVisitor;

impl<'de> Visitor<'de> for ClientMessageVisitor {
    type Value = ClientMessage;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a nostr client message array")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ClientMessage, A::Error> {
        let verb: String = required(&mut seq, 0)?;

        let message = match verb.as_str() {
            "EVENT" => ClientMessage::Event(required(&mut seq, 1)?),
            "AUTH" => ClientMessage::Auth(required(&mut seq, 1)?),
            "CLOSE" => ClientMessage::Close(required(&mut seq, 1)?),
            "NEG-CLOSE" => ClientMessage::NegClose(required(&mut seq, 1)?),
            "NEG-OPEN" => ClientMessage::NegOpen {
                subscription_id: required(&mut seq, 1)?,
                filter: required(&mut seq, 2)?,
                message: required(&mut seq, 3)?,
            },
            "NEG-MSG" => ClientMessage::NegMsg {
                subscription_id: required(&mut seq, 1)?,
                message: required(&mut seq, 2)?,
            },
            "REQ" | "COUNT" => {
                let subscription_id = required(&mut seq, 1)?;
                let mut filters = Vec::new();

                while let Some(filter) = seq.next_element()? {
                    filters.push(filter);
                }

                if filters.is_empty() {
                    return Err(de::Error::custom(format!(
                        "{verb} needs at least one filter"
                    )));
                }

                return Ok(if verb == "REQ" {
                    ClientMessage::Req {
                        subscription_id,
                        filters,
                    }
                } else {
                    ClientMessage::Count {
                        subscription_id,
                        filters,
                    }
                });
            }
            other => {
                return Err(de::Error::custom(format!(
                    "unknown client message type: {other}"
                )));
            }
        };

        // Trailing elements belong to later NIPs and are ignored.
        drain(&mut seq)?;

        Ok(message)
    }
}

/// Something the relay said back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayMessage {
    /// `["EVENT", <id>, <event>]` — an event belonging to a subscription.
    Event {
        subscription_id: SubscriptionId,
        event: RawEvent,
    },
    /// `["OK", <id>, <accepted>, <message>]` — the verdict on a published event,
    /// its message prefixed with a machine-readable reason when refused.
    Ok {
        event_id: EventId,
        accepted: bool,
        message: String,
    },
    /// `["EOSE", <id>]` — stored events are exhausted; what follows is live.
    Eose(SubscriptionId),
    /// `["COUNT", <id>, {"count": <count>}]` — how many events a count's
    /// filters match (NIP-45).
    Count {
        subscription_id: SubscriptionId,
        count: u64,
    },
    /// `["CLOSED", <id>, <message>]` — the relay ended a subscription.
    Closed {
        subscription_id: SubscriptionId,
        message: String,
    },
    /// `["NOTICE", <message>]` — a human-readable aside, tied to no
    /// subscription.
    Notice(String),
    /// `["AUTH", <challenge>]` — an auth challenge (NIP-42).
    Auth { challenge: String },
    /// `["NEG-MSG", <id>, <message>]` — one round of reconciliation
    /// (NIP-77).
    NegMsg {
        subscription_id: SubscriptionId,
        message: String,
    },
    /// `["NOTIFY", <message>]` — a message for the account the connection is
    /// authenticated as, rather than about the connection.
    Notify(String),
    /// `["NEG-ERR", <id>, <reason>]` — the reconciliation is over, for a reason
    /// prefixed as an `OK` message's is.
    NegErr {
        subscription_id: SubscriptionId,
        reason: String,
    },
}

impl RelayMessage {
    /// An event was accepted.
    pub fn accepted(event_id: EventId) -> Self {
        RelayMessage::Ok {
            event_id,
            accepted: true,
            message: String::new(),
        }
    }

    /// The verb this message goes out under, for logs and metrics.
    pub fn verb(&self) -> &'static str {
        match self {
            RelayMessage::Event { .. } => "EVENT",
            RelayMessage::Ok { .. } => "OK",
            RelayMessage::Eose(..) => "EOSE",
            RelayMessage::Count { .. } => "COUNT",
            RelayMessage::Closed { .. } => "CLOSED",
            RelayMessage::Notice(_) => "NOTICE",
            RelayMessage::Auth { .. } => "AUTH",
            RelayMessage::NegMsg { .. } => "NEG-MSG",
            RelayMessage::NegErr { .. } => "NEG-ERR",
            RelayMessage::Notify(_) => "NOTIFY",
        }
    }
}

impl Serialize for RelayMessage {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            RelayMessage::Event {
                subscription_id,
                event,
            } => {
                let mut seq = serializer.serialize_seq(Some(3))?;
                seq.serialize_element("EVENT")?;
                seq.serialize_element(subscription_id)?;
                seq.serialize_element(event)?;
                seq.end()
            }
            RelayMessage::Ok {
                event_id,
                accepted,
                message,
            } => {
                let mut seq = serializer.serialize_seq(Some(4))?;
                seq.serialize_element("OK")?;
                seq.serialize_element(event_id)?;
                seq.serialize_element(accepted)?;
                seq.serialize_element(message)?;
                seq.end()
            }
            RelayMessage::Eose(subscription_id) => {
                let mut seq = serializer.serialize_seq(Some(2))?;
                seq.serialize_element("EOSE")?;
                seq.serialize_element(subscription_id)?;
                seq.end()
            }
            RelayMessage::Count {
                subscription_id,
                count,
            } => {
                let mut seq = serializer.serialize_seq(Some(3))?;
                seq.serialize_element("COUNT")?;
                seq.serialize_element(subscription_id)?;
                seq.serialize_element(&serde_json::json!({ "count": count }))?;
                seq.end()
            }
            RelayMessage::Closed {
                subscription_id,
                message,
            } => {
                let mut seq = serializer.serialize_seq(Some(3))?;
                seq.serialize_element("CLOSED")?;
                seq.serialize_element(subscription_id)?;
                seq.serialize_element(message)?;
                seq.end()
            }
            RelayMessage::Notice(message) => {
                let mut seq = serializer.serialize_seq(Some(2))?;
                seq.serialize_element("NOTICE")?;
                seq.serialize_element(message)?;
                seq.end()
            }
            RelayMessage::Auth { challenge } => {
                let mut seq = serializer.serialize_seq(Some(2))?;
                seq.serialize_element("AUTH")?;
                seq.serialize_element(challenge)?;
                seq.end()
            }
            RelayMessage::NegMsg {
                subscription_id,
                message,
            } => {
                let mut seq = serializer.serialize_seq(Some(3))?;
                seq.serialize_element("NEG-MSG")?;
                seq.serialize_element(subscription_id)?;
                seq.serialize_element(message)?;
                seq.end()
            }
            RelayMessage::NegErr {
                subscription_id,
                reason,
            } => {
                let mut seq = serializer.serialize_seq(Some(3))?;
                seq.serialize_element("NEG-ERR")?;
                seq.serialize_element(subscription_id)?;
                seq.serialize_element(reason)?;
                seq.end()
            }
            RelayMessage::Notify(message) => {
                let mut seq = serializer.serialize_seq(Some(2))?;
                seq.serialize_element("NOTIFY")?;
                seq.serialize_element(message)?;
                seq.end()
            }
        }
    }
}

/// The element at `index`, or an error naming the message that was too
/// short.
fn required<'de, T, A>(seq: &mut A, index: usize) -> Result<T, A::Error>
where
    T: Deserialize<'de>,
    A: SeqAccess<'de>,
{
    seq.next_element()?
        .ok_or_else(|| de::Error::invalid_length(index, &"a longer nostr message"))
}

/// Consume whatever is left, so trailing elements are ignored rather
/// than fatal.
fn drain<'de, A: SeqAccess<'de>>(seq: &mut A) -> Result<(), A::Error> {
    while seq.next_element::<IgnoredAny>()?.is_some() {}

    Ok(())
}

#[cfg(test)]
mod tests {
    use coracle_lib::events::{EventContent, HasTags};
    use coracle_lib::keys::SecretKey;
    use coracle_lib::tags::Tags;

    use super::*;

    /// A signed event, so round trips exercise the same parsing path a
    /// published event takes.
    fn event() -> Event {
        let secret = SecretKey::generate();

        let hashed = EventContent::new()
            .with_content("hello")
            .with_kind(1)
            .with_created_at(1_700_000_000)
            .with_pubkey(secret.public_key())
            .with_id();

        let sig = secret.sign(hashed.id.as_bytes());

        hashed.with_sig(sig)
    }

    fn parse(json: &str) -> ClientMessage {
        serde_json::from_str(json).expect("parses")
    }

    #[test]
    fn reads_an_event() {
        let event = event();
        let json = format!("[\"EVENT\",{}]", serde_json::to_string(&event).unwrap());

        assert_eq!(parse(&json), ClientMessage::Event(event));
    }

    #[test]
    fn a_round_tripped_event_still_verifies() {
        let json = format!("[\"EVENT\",{}]", serde_json::to_string(&event()).unwrap());

        let ClientMessage::Event(parsed) = parse(&json) else {
            panic!("expected an EVENT");
        };

        assert!(parsed.verify().is_ok());
    }

    #[test]
    fn reads_a_req_with_several_filters() {
        let message = parse(r#"["REQ","sub",{"kinds":[1]},{"kinds":[7],"limit":10}]"#);

        let ClientMessage::Req {
            subscription_id,
            filters,
        } = message
        else {
            panic!("expected a REQ");
        };

        assert_eq!(subscription_id, SubscriptionId::new("sub").unwrap());
        assert_eq!(filters.len(), 2);
        assert_eq!(filters[1].limit, Some(10));
    }

    #[test]
    fn refuses_a_req_with_no_filters() {
        assert!(serde_json::from_str::<ClientMessage>(r#"["REQ","sub"]"#).is_err());
    }

    #[test]
    fn reads_a_count() {
        let message = parse(r#"["COUNT","sub",{"kinds":[7]}]"#);

        assert!(matches!(message, ClientMessage::Count { filters, .. } if filters.len() == 1));
    }

    #[test]
    fn reads_a_close() {
        assert_eq!(
            parse(r#"["CLOSE","sub"]"#),
            ClientMessage::Close(SubscriptionId::new("sub").unwrap())
        );
    }

    #[test]
    fn ignores_elements_past_the_ones_it_knows() {
        assert_eq!(
            parse(r#"["CLOSE","sub","from a nip that came later"]"#),
            ClientMessage::Close(SubscriptionId::new("sub").unwrap())
        );
    }

    #[test]
    fn refuses_an_unknown_verb() {
        assert!(serde_json::from_str::<ClientMessage>(r#"["SHOUT","hello"]"#).is_err());
    }

    #[test]
    fn refuses_a_message_that_is_not_an_array() {
        assert!(serde_json::from_str::<ClientMessage>(r#"{"type":"EVENT"}"#).is_err());
    }

    #[test]
    fn refuses_a_subscription_id_of_the_wrong_length() {
        assert!(SubscriptionId::new("").is_err());
        assert!(SubscriptionId::new("a".repeat(MAX_SUBSCRIPTION_ID_LEN)).is_ok());
        assert!(SubscriptionId::new("a".repeat(MAX_SUBSCRIPTION_ID_LEN + 1)).is_err());
        assert!(serde_json::from_str::<ClientMessage>(r#"["CLOSE",""]"#).is_err());
    }

    #[test]
    fn writes_an_ok() {
        let event = event();
        let message = RelayMessage::accepted(event.id);

        assert_eq!(
            serde_json::to_string(&message).unwrap(),
            format!("[\"OK\",\"{}\",true,\"\"]", event.id.to_hex())
        );
    }

    #[test]
    fn writes_an_eose() {
        let message = RelayMessage::Eose(SubscriptionId::new("sub").unwrap());

        assert_eq!(
            serde_json::to_string(&message).unwrap(),
            r#"["EOSE","sub"]"#
        );
    }

    #[test]
    fn writes_a_count() {
        let message = RelayMessage::Count {
            subscription_id: SubscriptionId::new("c").unwrap(),
            count: 42,
        };

        assert_eq!(
            serde_json::to_string(&message).unwrap(),
            r#"["COUNT","c",{"count":42}]"#
        );
    }

    #[test]
    fn writes_a_notice() {
        assert_eq!(
            serde_json::to_string(&RelayMessage::Notice("careful".to_string())).unwrap(),
            r#"["NOTICE","careful"]"#
        );
    }

    #[test]
    fn an_event_keeps_its_tags_through_a_round_trip() {
        let secret = SecretKey::generate();

        let hashed = EventContent::new()
            .with_content("hello")
            .with_tags(Tags::new().add("e", ["abc", "wss://relay.example"]))
            .with_kind(1)
            .with_created_at(1_700_000_000)
            .with_pubkey(secret.public_key())
            .with_id();

        let sig = secret.sign(hashed.id.as_bytes());
        let event = hashed.with_sig(sig);
        let json = format!("[\"EVENT\",{}]", serde_json::to_string(&event).unwrap());

        let ClientMessage::Event(parsed) = parse(&json) else {
            panic!("expected an EVENT");
        };

        assert_eq!(
            parsed.tags().find("e").unwrap().as_slice(),
            ["e", "abc", "wss://relay.example"]
        );
        assert!(parsed.verify().is_ok());
    }
}
