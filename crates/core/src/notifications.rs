//! Notifications: one message per pubkey per relay, which the relay sends as a
//! `NOTIFY` to every connection authenticated as that pubkey, now and whenever it
//! authenticates again, until the message is replaced, cleared or expires.

use crate::Core;
use caravel_db::commands;
use caravel_util::bus::Change;
use caravel_util::error::{Error, Result};

/// Replace a pubkey's message on a relay, and send it to them now if they are
/// connected and it hasn't expired.
pub async fn notify(
    core: &Core,
    relay_id: &str,
    pubkey: &str,
    message: &str,
    expires_at: Option<i64>,
) -> Result<()> {
    if message.is_empty() {
        return Err(Error::invalid("message-required", "message is required"));
    }

    let now = core.clock.now();
    commands::notifications::set_notification(
        core.db.pool(),
        relay_id,
        pubkey,
        message,
        expires_at,
        now,
    )
    .await?;

    if expires_at.is_none_or(|expires_at| expires_at > now) {
        core.bus.publish(Change::Notified {
            relay_id: relay_id.to_string(),
            pubkey: pubkey.to_string(),
            message: message.to_string(),
        });
    }
    Ok(())
}
