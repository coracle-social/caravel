//! Keeping the event store tidy: sweeping expired events, and dropping push
//! subscriptions nobody can be reached at.

use crate::Core;
use crate::bus::Change;
use crate::commands;
use crate::error::Result;

/// Delete every relay's events whose NIP-40 expiration passed, a batch at a time
/// so other writers get between them.
pub async fn sweep_expired_events(core: &Core) -> Result<()> {
    const BATCH: i64 = 1000;

    loop {
        let removed =
            commands::events::delete_expired_events(core.db.pool(), core.clock.now(), BATCH)
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
    commands::events::delete_event(core.db.pool(), relay_id, id).await?;
    core.bus.publish(Change::AccessChanged {
        relay_id: relay_id.to_string(),
        published: Vec::new(),
    });
    Ok(())
}
