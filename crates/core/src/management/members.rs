//! Adding and removing a relay's members from the dashboards.

use caravel_billing::relays::Relay;
use caravel_util::error::Result;

use caravel_relays::access::MemberSource;
use caravel_util::bus::Change;
use caravel_util::encoding::is_hex64;
use caravel_util::error::Error;

use crate::Core;

/// Make a pubkey a member of a relay by hand.
pub async fn add_member(core: &Core, relay: &Relay, pubkey: &str) -> Result<()> {
    relay.require_unpurged()?;
    if !is_hex64(pubkey) {
        return Err(Error::invalid(
            "invalid-pubkey",
            "pubkey must be 64 hex characters",
        ));
    }

    let pubkey = pubkey.to_lowercase();
    let key = core.relay_key(relay)?;
    let now = core.clock.now();
    let published = core
        .db
        .with_tx(async |conn| {
            caravel_relays::access::commands::admit(
                conn,
                &key,
                &relay.id,
                &pubkey,
                MemberSource::Manual,
                now,
            )
            .await
        })
        .await?;

    core.bus.publish(Change::AccessChanged {
        relay_id: relay.id.clone(),
        published,
    });
    Ok(())
}

/// Remove a member from a relay. Its owner can't be removed.
pub async fn remove_member(core: &Core, relay: &Relay, pubkey: &str) -> Result<()> {
    relay.require_unpurged()?;
    let key = core.relay_key(relay)?;
    let now = core.clock.now();
    let published = core
        .db
        .with_tx(async |conn| {
            caravel_relays::access::commands::remove(
                conn,
                &key,
                &relay.id,
                &relay.tenant_pubkey,
                pubkey,
                now,
            )
            .await
        })
        .await?;

    core.bus.publish(Change::AccessChanged {
        relay_id: relay.id.clone(),
        published,
    });
    Ok(())
}
