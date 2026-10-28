//! Keeping the event store tidy: sweeping expired events, and dropping push
//! subscriptions nobody can be reached at.

use coracle_lib::filters::Filter;

use crate::Core;
use caravel_billing::relays::Relay;
use caravel_relays::events::{EventGroup, EventHead, StoredEvent};
use caravel_relays::notify::Notification;
use caravel_relays::policy::RelaySnapshot;
use caravel_util::bus::Change;
use caravel_util::error::Result;

/// A relay's unexpired events matching `filter`, newest first, for a client's
/// subscription.
pub async fn query_events(
    core: &Core,
    relay_id: &str,
    filter: &Filter,
    max_limit: usize,
    now: i64,
) -> Result<Vec<StoredEvent>> {
    caravel_relays::events::queries::query_events(core.db.pool(), relay_id, filter, max_limit, now)
        .await
}

/// The head of each group of a relay's events matching the filters, for a sync.
pub async fn query_event_heads(
    core: &Core,
    relay_id: &str,
    filter: &Filter,
    max_limit: usize,
    now: i64,
) -> Result<Option<Vec<EventHead>>> {
    caravel_relays::events::queries::query_event_heads(
        core.db.pool(),
        relay_id,
        filter,
        max_limit,
        now,
    )
    .await
}

/// How many groups of events the filters name, which bounds what a count answers.
pub async fn count_event_groups(
    core: &Core,
    relay_id: &str,
    filters: &[Filter],
    now: i64,
) -> Result<Vec<EventGroup>> {
    caravel_relays::events::queries::count_event_groups(core.db.pool(), relay_id, filters, now)
        .await
}

/// The notification waiting for a pubkey on a relay, if there is one.
pub async fn get_notification(
    core: &Core,
    relay_id: &str,
    pubkey: &str,
    now: i64,
) -> Result<Option<Notification>> {
    caravel_relays::notify::queries::get_notification(core.db.pool(), relay_id, pubkey, now).await
}

/// What a relay's rules read: its settings, then its access lists and rooms.
pub async fn load_snapshot(core: &Core, relay: &Relay) -> Result<RelaySnapshot> {
    caravel_relays::policy::queries::load_snapshot(core.db.pool(), relay.snapshot()).await
}

/// Delete every relay's events whose NIP-40 expiration passed, a batch at a time
/// so other writers get between them.
pub async fn sweep_expired_events(core: &Core) -> Result<()> {
    const BATCH: i64 = 1000;

    loop {
        let removed = caravel_relays::events::commands::delete_expired_events(
            core.db.pool(),
            core.clock.now(),
            BATCH,
        )
        .await?;
        if removed > 0 {
            tracing::info!(removed, "swept expired events");
        }
        if removed < BATCH as u64 {
            return Ok(());
        }
    }
}

/// Drop a push subscription whose callback can't be reached, and reload its relay.
pub async fn drop_push_subscription(core: &Core, relay_id: &str, id: &[u8; 32]) -> Result<()> {
    caravel_relays::events::commands::delete_event(core.db.pool(), relay_id, id).await?;
    core.bus.publish(Change::AccessChanged {
        relay_id: relay_id.to_string(),
        published: Vec::new(),
    });
    Ok(())
}
