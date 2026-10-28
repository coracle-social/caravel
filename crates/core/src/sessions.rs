//! Sessions: the bearer tokens a NIP-98 signature buys, good for thirty days.

use crate::Core;
use caravel_operations::sessions::Session;
use caravel_util::error::Result;

/// How stale a session's `last_used_at` may get before a request refreshes it.
const TOUCH_INTERVAL: i64 = 60 * 60;

/// Start a session for `pubkey`, returning the bearer token that stands for it.
/// Only the token's hash is stored.
pub async fn start(core: &Core, pubkey: &str) -> Result<(String, Session)> {
    caravel_operations::sessions::commands::create_session(core.db.pool(), pubkey, core.clock.now())
        .await
}

/// The live session a bearer token stands for, refreshing its `last_used_at` at
/// most once an hour.
pub async fn find(core: &Core, token: &str) -> Result<Option<Session>> {
    let now = core.clock.now();
    let session =
        caravel_operations::sessions::queries::get_session(core.db.pool(), token, now).await?;

    if let Some(session) = &session
        && now - session.last_used_at > TOUCH_INTERVAL
    {
        caravel_operations::sessions::commands::touch_session(core.db.pool(), token, now).await?;
    }

    Ok(session)
}

/// End the session a bearer token stands for.
pub async fn end(core: &Core, token: &str) -> Result<()> {
    caravel_operations::sessions::commands::delete_session(core.db.pool(), token).await
}

/// End every session `pubkey` holds.
pub async fn revoke(core: &Core, pubkey: &str) -> Result<u64> {
    caravel_operations::sessions::commands::revoke_sessions(core.db.pool(), pubkey).await
}

/// Delete the sessions that have expired.
pub async fn sweep(core: &Core) -> Result<u64> {
    caravel_operations::sessions::commands::delete_expired_sessions(
        core.db.pool(),
        core.clock.now(),
    )
    .await
}
