//! NIP-9a push: members register kind 30390 subscriptions naming filters and a
//! callback, and the relay POSTs the id of each new matching event to the callback.
//! Subscriptions are stored as events and indexed with each relay's state;
//! deliveries go through a bounded pool so a slow callback holds up nothing else.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use caravel_core::models::events::StoredEvent;
use caravel_core::models::kinds;
use caravel_core::models::policy::{EventFacts, RelaySnapshot};
use caravel_core::{Core, domain, events, queries};
use coracle_lib::events::{Event, EventId};
use coracle_lib::filters::{Filter, matches_any};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// How many deliveries run at once.
const WORKERS: usize = 16;

/// How many deliveries wait for a worker before new ones are dropped.
const QUEUE: usize = 4096;

/// Consecutive failures after which a subscription is dropped.
const MAX_FAILURES: u32 = 10;

/// The most subscriptions read for one relay.
const MAX_SUBSCRIPTIONS: usize = 100_000;

/// A stored push subscription, parsed.
#[derive(Clone, Debug)]
pub struct Subscription {
    pub id: EventId,
    pub pubkey: String,
    pub d: String,
    pub filters: Vec<Filter>,
    pub ignore: Vec<Filter>,
    pub callback: String,
    pub include_event: bool,
}

impl Subscription {
    fn parse(stored: &StoredEvent) -> Option<Self> {
        let event: Event = serde_json::from_str(&stored.to_json(false)).ok()?;
        let parse_filters = |name: &str| -> Vec<Filter> {
            event
                .tags
                .values(name)
                .filter_map(|filter| serde_json::from_str(filter).ok())
                .collect()
        };

        Some(Self {
            id: event.id,
            pubkey: event.pubkey.to_hex(),
            d: event.tags.value("d")?.to_string(),
            filters: parse_filters("filter"),
            ignore: parse_filters("ignore"),
            callback: event.tags.value("callback")?.to_string(),
            include_event: event.tags.find("include_event").is_some(),
        })
    }
}

/// A relay's push subscriptions.
pub async fn load_subscriptions(
    core: &Core,
    relay_id: &str,
) -> caravel_core::Result<Vec<Subscription>> {
    let filter = Filter {
        kinds: Some([kinds::PUSH_SUBSCRIPTION].into()),
        ..Default::default()
    };
    let stored = queries::events::query_events(
        core.db.pool(),
        relay_id,
        &filter,
        MAX_SUBSCRIPTIONS,
        core.clock.now(),
    )
    .await?;

    Ok(stored.iter().filter_map(Subscription::parse).collect())
}

/// How many subscriptions an author holds besides the one `event` replaces.
pub fn held_by(subscriptions: &[Subscription], event: &Event) -> usize {
    let author = event.pubkey.to_hex();
    let d = event.tags.value("d").unwrap_or_default();
    subscriptions
        .iter()
        .filter(|subscription| subscription.pubkey == author && subscription.d != d)
        .count()
}

/// One callback to call.
struct Delivery {
    relay_id: String,
    subscription: EventId,
    callback: String,
    payload: Value,
}

/// The pool deliveries go through, and each subscription's run of failures.
#[derive(Clone)]
pub struct Pusher {
    sender: mpsc::Sender<Delivery>,
    /// By relay id and subscription.
    failures: Arc<Mutex<HashMap<(String, EventId), u32>>>,
}

impl Pusher {
    /// Start the delivery workers, which stop at `shutdown`.
    pub fn start(core: Core, shutdown: CancellationToken) -> Self {
        let (sender, receiver) = mpsc::channel(QUEUE);
        let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
        let pusher = Self {
            sender,
            failures: Arc::default(),
        };

        for _ in 0..WORKERS {
            let pusher = pusher.clone();
            let core = core.clone();
            let receiver = receiver.clone();
            let shutdown = shutdown.clone();
            tokio::spawn(async move {
                loop {
                    let delivery = tokio::select! {
                        _ = shutdown.cancelled() => break,
                        delivery = async { receiver.lock().await.recv().await } => delivery,
                    };
                    let Some(delivery) = delivery else { break };
                    pusher.deliver(&core, delivery).await;
                }
            });
        }

        pusher
    }

    /// Queue a callback for each subscription that matches a new event and whose
    /// author may read it, skipping the event's own author. The event is included,
    /// stripped where the relay strips signatures, for subscriptions that ask.
    pub fn push(
        &self,
        snapshot: &RelaySnapshot,
        subscriptions: &[Subscription],
        primary_host: &str,
        event: &Event,
    ) {
        let author = event.pubkey.to_hex();
        let facts = EventFacts::of(event, &author);
        let stored = StoredEvent::from_event(event);

        for subscription in subscriptions {
            let reader = [subscription.pubkey.clone()];
            if subscription.pubkey == author
                || !domain::policy::can_deliver(snapshot, &reader, facts)
                || !matches_any(&subscription.filters, event)
                || matches_any(&subscription.ignore, event)
            {
                continue;
            }

            let mut payload = json!({
                "id": event.id.to_hex(),
                "relay": format!("wss://{primary_host}/"),
            });
            if subscription.include_event {
                let strip = domain::policy::strips_signature(snapshot, &reader);
                payload["event"] = serde_json::from_str(&stored.to_json(strip)).unwrap_or_default();
            }

            let delivery = Delivery {
                relay_id: snapshot.relay_id.clone(),
                subscription: subscription.id,
                callback: subscription.callback.clone(),
                payload,
            };
            if self.sender.try_send(delivery).is_err() {
                tracing::warn!(relay = %snapshot.relay_id, "dropped a push delivery");
            }
        }
    }

    /// Forget the failures of a relay's subscriptions other than `held`, its
    /// current ones, once the rest have been replaced, deleted or expired.
    pub fn retain(&self, relay_id: &str, held: &[Subscription]) {
        let held: HashSet<EventId> = held.iter().map(|subscription| subscription.id).collect();
        self.failures()
            .retain(|(relay, subscription), _| relay != relay_id || held.contains(subscription));
    }

    /// Call one callback. A 404 drops the subscription at once; other failures
    /// drop it once they run to ten in a row.
    async fn deliver(&self, core: &Core, delivery: Delivery) {
        let status = core
            .outbound
            .post_json(&delivery.callback, &delivery.payload)
            .await
            .ok();

        let key = (delivery.relay_id.clone(), delivery.subscription);
        let drop = match status {
            Some(status) if (200..300).contains(&status) => {
                self.failures().remove(&key);
                false
            }
            Some(404) => {
                self.failures().remove(&key);
                true
            }
            _ => {
                let mut failures = self.failures();
                let count = failures.entry(key.clone()).or_default();
                *count += 1;
                let exhausted = *count >= MAX_FAILURES;
                if exhausted {
                    failures.remove(&key);
                }
                exhausted
            }
        };

        if drop {
            tracing::info!(relay = %delivery.relay_id, subscription = %delivery.subscription.to_hex(), "dropping a push subscription");
            let id = delivery.subscription.as_bytes();
            if let Err(error) = events::drop_push_subscription(core, &delivery.relay_id, id).await {
                tracing::warn!(relay = %delivery.relay_id, error = %error, "failed to drop a push subscription");
            }
        }
    }

    fn failures(&self) -> std::sync::MutexGuard<'_, HashMap<(String, EventId), u32>> {
        self.failures
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subscription(id: u8) -> Subscription {
        Subscription {
            id: EventId::new([id; 32]),
            pubkey: String::new(),
            d: id.to_string(),
            filters: Vec::new(),
            ignore: Vec::new(),
            callback: String::new(),
            include_event: false,
        }
    }

    #[test]
    fn failures_are_forgotten_once_their_subscription_is_gone() {
        let pusher = Pusher {
            sender: mpsc::channel(1).0,
            failures: Arc::default(),
        };
        let (kept, replaced) = (subscription(1), subscription(2));
        for (relay_id, id) in [("a", kept.id), ("a", replaced.id), ("b", replaced.id)] {
            pusher.failures().insert((relay_id.to_string(), id), 3);
        }

        pusher.retain("a", std::slice::from_ref(&kept));
        let mut left: Vec<_> = pusher.failures().keys().cloned().collect();
        left.sort();
        let expected = [("a".to_string(), kept.id), ("b".to_string(), replaced.id)];
        assert_eq!(left, expected);

        pusher.retain("b", &[]);
        assert_eq!(pusher.failures().len(), 1);
    }
}
