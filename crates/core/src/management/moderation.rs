//! Moderating a room as the relay itself, from the dashboards.

use caravel_billing::relays::Relay;
use caravel_util::error::Result;

use caravel_util::bus::Change;
use caravel_util::error::Error;

use crate::Core;

/// Publish a NIP-29 moderation event as the relay itself, for whoever manages it
/// from the dashboards, and apply it to its room. The relay may publish any
/// moderation kind, so only the room rules on what the event says can refuse it.
pub async fn moderate_room(
    core: &Core,
    relay: &Relay,
    kind: u16,
    tags: Vec<Vec<String>>,
) -> Result<()> {
    relay.require_unpurged()?;
    if !caravel_relays::rooms::is_moderation_kind(kind) {
        return Err(Error::invalid(
            "invalid-kind",
            format!("{kind} is not a group management kind"),
        ));
    }

    let key = core.relay_key(relay)?;
    let now = core.clock.now();
    let event = key.sign(kind, tags, "", now);
    let snapshot =
        caravel_relays::policy::queries::load_snapshot(core.db.pool(), relay.snapshot()).await?;
    snapshot
        .check_room_write(&event)
        .map_err(|refusal| Error::invalid("refused", refusal.message()))?;

    let mut published = core
        .db
        .with_tx(async |conn| {
            caravel_relays::events::commands::save_event(&mut *conn, &relay.id, &event, now)
                .await?;
            caravel_relays::rooms::commands::apply_event(conn, &key, &relay.id, &event, now).await
        })
        .await?;
    published.insert(0, event);

    core.bus.publish(Change::AccessChanged {
        relay_id: relay.id.clone(),
        published,
    });
    Ok(())
}
