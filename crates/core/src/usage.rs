//! Metering usage: hourly readings of each paid relay's storage and A/V call time.

use crate::Core;
use caravel_db::{commands, queries};
use caravel_domain::billing;
use caravel_util::error::Result;
use caravel_domain::usage::UsageSample;

/// Take a usage reading of every active relay on a paid plan with a metered
/// feature on: the blobs it holds, and its call clock.
pub async fn sample_usage(core: &Core) -> Result<()> {
    let now = core.clock.now();

    for relay in queries::relays::list_relays(core.db.pool()).await? {
        if !relay.is_served() || (!relay.blossom_enabled && !relay.livekit_enabled) {
            continue;
        }
        if !billing::get_plan(&relay.plan_id).is_some_and(|plan| plan.is_paid()) {
            continue;
        }

        let sample = UsageSample {
            id: uuid::Uuid::new_v4().to_string(),
            relay_id: relay.id.clone(),
            tenant_pubkey: relay.tenant_pubkey.clone(),
            storage_bytes: queries::media::storage_bytes(core.db.pool(), &relay.id).await?,
            participant_seconds: queries::calls::participant_seconds(core.db.pool(), &relay.id)
                .await?,
            created_at: now,
        };

        commands::usage::insert_usage_sample(core.db.pool(), &sample).await?;
    }

    Ok(())
}
