//! Acting on what clients publish to a relay: keeping events along with the
//! changes they make, and NIP-43 joins and leaves. The relay runtime calls these
//! after its rules admit the event, and applies what the relay published itself
//! before answering the client, so nothing here goes on the bus.

use coracle_lib::events::Event;

use crate::Core;
use caravel_db::commands;
use caravel_domain as domain;
use caravel_domain::error::{Error, Result};
use caravel_domain::events::Saved;
use caravel_domain::policy::Join;
use caravel_domain::relays::Relay;
use caravel_util::nostr::Key;
use caravel_util::nostr::kinds::{DELETION, VANISH};

/// Keep an event, applying a deletion, a request to vanish, or a change to a room
/// in the same transaction. Returns how it was saved, and what the relay
/// published about the change.
pub async fn store(
    core: &Core,
    relay: &Relay,
    key: &Key,
    rooms_enabled: bool,
    event: &Event,
) -> Result<(Saved, Vec<Event>)> {
    let relay_id = relay.id.as_str();
    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            let saved = commands::events::save_event(&mut *conn, relay_id, event, now).await?;
            let mut published = Vec::new();
            if saved == Saved::New {
                match event.kind {
                    DELETION => {
                        commands::events::apply_deletion(conn, relay_id, event).await?;
                    }
                    VANISH => {
                        commands::events::apply_vanish(
                            conn,
                            relay_id,
                            &event.pubkey.as_bytes(),
                            event.created_at,
                        )
                        .await?;
                    }
                    kind if rooms_enabled
                        && domain::rooms::is_room_event(kind, domain::rooms::room_id(event)) =>
                    {
                        published =
                            commands::rooms::apply_event(conn, key, relay_id, event, now).await?;
                    }
                    _ => {}
                }
            }
            Ok::<_, Error>((saved, published))
        })
        .await
}

/// Admit a NIP-43 join request the relay's rules let through, as `join` says, and
/// return what the relay published.
pub async fn join(
    core: &Core,
    relay: &Relay,
    key: &Key,
    event: &Event,
    join: &Join,
) -> Result<Vec<Event>> {
    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            commands::memberships::admit_join(conn, key, relay, event, join, now).await
        })
        .await
}

/// Act on a NIP-43 leave request, returning what the relay published. The owner
/// can't leave.
pub async fn leave(core: &Core, relay: &Relay, key: &Key, pubkey: &str) -> Result<Vec<Event>> {
    let now = core.clock.now();
    core.db
        .with_tx(async |conn| {
            commands::memberships::remove(conn, key, &relay.id, &relay.tenant_pubkey, pubkey, now)
                .await
        })
        .await
}
