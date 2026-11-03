//! Reading the event store for the relay runtime: a subscription's events, a sync's
//! heads, what a count names and the notification waiting for a peer.

use caravel_util::nostr::keys::parse_pubkey;
use coracle_lib::filters::Filter;

use crate::Core;
use caravel_billing::relays::Relay;
use caravel_relays::events::{EventGroup, EventHead, StoredEvent};
use caravel_relays::notify::Notification;
use caravel_relays::policy::RelaySnapshot;
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
    let relay_pubkey = parse_pubkey(&relay.pubkey)?;
    let tenant_pubkey = parse_pubkey(&relay.tenant_pubkey)?;
    let snapshot = RelaySnapshot {
        public_read: relay.policy_public_read,
        anonymous_read: relay.policy_anonymous_read,
        public_write: relay.policy_public_write,
        public_join: relay.policy_public_join,
        strip_signatures: relay.policy_strip_signatures,
        groups_enabled: relay.groups_enabled,
        members_can_invite: relay.members_can_invite,
        charges_members: relay.charges_members(),
        member_limit: relay
            .member_limit()
            .and_then(|limit| usize::try_from(limit).ok()),
        ..RelaySnapshot::new(relay.id.clone(), relay_pubkey, tenant_pubkey)
    };

    caravel_relays::policy::queries::load_snapshot(core.db.pool(), snapshot).await
}
