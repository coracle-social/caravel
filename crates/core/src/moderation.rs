//! The moderation feed admins review: what moderated relays saw, across tenants.

use crate::Core;
use caravel_operations::moderation::{ModerationPage, queries};
use caravel_util::error::Result;

/// The feed of events and uploads moderated relays saw, newest first. Page back by
/// moving `until` to the last item's `seen_at`.
pub async fn list_feed(
    core: &Core,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<ModerationPage> {
    queries::list_moderation(core.db.pool(), &core.config.admin_pubkeys, since, until).await
}
