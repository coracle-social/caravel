//! Erasing what a purged relay held: its events, media, rooms and access lists.

use caravel_billing::relays::Relay;
use caravel_util::bus::Change;
use caravel_util::error::{Error, Result};

use crate::Core;

/// Erase what a relay holds, keeping its settings: its events, media, rooms, roles,
/// method grants, bans, claims, notifications and members. Its owner stays, as do
/// the members and invites someone has paid for, and the relay republishes its
/// member list.
pub async fn erase_relay(core: &Core, relay: &Relay) -> Result<()> {
    relay.require_unpurged()?;

    let key = core.relay_key(relay)?;
    let now = core.clock.now();
    let (published, blobs) = core
        .db
        .with_tx(async |conn| {
            // Joins bind paid invites under this lock.
            caravel_billing::relays::queries::lock_relay(conn, &relay.id).await?;

            let blobs =
                caravel_billing::relays::commands::erase_relay_data(&mut *conn, &relay.id).await?;
            let published =
                caravel_billing::relays::commands::rebuild_derived(conn, &key, &relay.id, now)
                    .await?;
            Ok::<_, Error>((published, blobs))
        })
        .await?;

    core.bus.publish(Change::AccessChanged {
        relay_id: relay.id.clone(),
        published,
    });

    // Bytes left behind go when the relay is purged.
    for sha256 in blobs {
        let key = caravel_relays::media::blob_key(&relay.id, &sha256);
        // A same-blob upload since the commit keeps its bytes; uploads hold this lock.
        let _lock = core.blobs.lock(&key).await;
        let gone = matches!(
            caravel_relays::media::queries::get_blob(core.db.pool(), &relay.id, &sha256).await,
            Ok(None)
        );
        if gone && let Err(error) = core.blobs.delete(&key).await {
            tracing::warn!(relay = %relay.id, error = %error, "failed to delete a blob's bytes");
        }
    }
    Ok(())
}

/// Erase what purged relays held: their events, access lists, rooms, claims,
/// notifications and media. Billing's records of them stay.
pub async fn erase_purged_relays(core: &Core) -> Result<()> {
    for relay in caravel_billing::relays::queries::list_unerased_relays(core.db.pool()).await? {
        core.blobs.delete_prefix(&relay.id).await?;

        let now = core.clock.now();
        core.db
            .with_tx(async |conn| {
                caravel_billing::relays::commands::erase_purged_relay_data(&mut *conn, &relay.id)
                    .await?;
                caravel_billing::relays::commands::set_relay_erased_at(conn, &relay.id, now).await
            })
            .await?;

        tracing::info!(relay = %relay.id, "erased a purged relay");
    }
    Ok(())
}
