//! Recording usage readings.

use sqlx::PgExecutor;

use caravel_util::error::Result;
use caravel_domain::usage::UsageSample;

pub async fn insert_usage_sample(db: impl PgExecutor<'_>, sample: &UsageSample) -> Result<()> {
    sqlx::query(
        "INSERT INTO usage_sample (id, relay_id, tenant_pubkey, storage_bytes, participant_seconds, created_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&sample.id)
    .bind(&sample.relay_id)
    .bind(&sample.tenant_pubkey)
    .bind(sample.storage_bytes)
    .bind(sample.participant_seconds)
    .bind(sample.created_at)
    .execute(db)
    .await?;
    Ok(())
}
