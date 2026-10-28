//! Metering usage: hourly readings of each paid relay's storage and A/V call time.

use crate::Core;
use caravel_billing::relays::UsageSample;
use caravel_billing::relays::plans as billing;
use caravel_util::error::Result;

/// Take a usage reading of every active relay on a paid plan with a metered
/// feature on: the blobs it holds, and its call clock.
pub async fn sample_usage(core: &Core) -> Result<()> {
    let now = core.clock.now();

    for relay in caravel_billing::relays::queries::list_relays(core.db.pool()).await? {
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
            storage_bytes: caravel_relays::media::queries::storage_bytes(core.db.pool(), &relay.id)
                .await?,
            participant_seconds: caravel_relays::calls::queries::participant_seconds(
                core.db.pool(),
                &relay.id,
            )
            .await?,
            created_at: now,
        };

        caravel_billing::relays::commands::usage::insert_usage_sample(core.db.pool(), &sample)
            .await?;
    }

    Ok(())
}
