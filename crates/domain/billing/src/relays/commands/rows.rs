//! Writing a relay's row, its info and its status.

use sqlx::{PgConnection, PgExecutor};

use caravel_util::error::{Error, Result};

use crate::relays::{Relay, RelayStatus};

/// Insert a relay. A taken subdomain is the caller's to fix.
pub async fn insert_relay(conn: &mut PgConnection, relay: &Relay) -> Result<()> {
    sqlx::query(
        "INSERT INTO relay (
            id, tenant_pubkey, subdomain, domain, plan_id, status, pubkey, secret,
            info_name, info_icon, info_description,
            policy_public_read, policy_public_write, policy_public_join, policy_strip_signatures,
            groups_enabled, management_enabled, members_can_invite, blossom_enabled,
            livekit_enabled, push_enabled, custom_domain, created_at, policy_anonymous_read,
            blossom_authenticated_read, member_price, member_interval
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17,
            $18, $19, $20, $21, $22, $23, $24, $25, $26, $27)",
    )
    .bind(&relay.id)
    .bind(&relay.tenant_pubkey)
    .bind(&relay.subdomain)
    .bind(&relay.domain)
    .bind(&relay.plan_id)
    .bind(relay.status)
    .bind(&relay.pubkey)
    .bind(&relay.secret)
    .bind(&relay.info_name)
    .bind(&relay.info_icon)
    .bind(&relay.info_description)
    .bind(relay.policy_public_read)
    .bind(relay.policy_public_write)
    .bind(relay.policy_public_join)
    .bind(relay.policy_strip_signatures)
    .bind(relay.groups_enabled)
    .bind(relay.management_enabled)
    .bind(relay.members_can_invite)
    .bind(relay.blossom_enabled)
    .bind(relay.livekit_enabled)
    .bind(relay.push_enabled)
    .bind(&relay.custom_domain)
    .bind(relay.created_at)
    .bind(relay.policy_anonymous_read)
    .bind(relay.blossom_authenticated_read)
    .bind(relay.member_price)
    .bind(relay.member_interval)
    .execute(conn)
    .await
    .map_err(|error| map_relay_write_error(error.into()))?;
    Ok(())
}

/// Write a relay's settings. Its redirect hosts, status, keys and purge marker
/// have writers of their own. Verification of its custom domain is kept unless
/// the custom domain or the canonical host it must point at changed, comparing
/// against the stored row so a verification that landed since the relay was read
/// survives.
pub async fn update_relay(conn: &mut PgConnection, relay: &Relay) -> Result<()> {
    sqlx::query(
        "UPDATE relay SET
            subdomain = $2, domain = $3, plan_id = $4,
            info_name = $5, info_icon = $6, info_description = $7,
            policy_public_read = $8, policy_public_write = $9,
            policy_public_join = $10, policy_strip_signatures = $11,
            groups_enabled = $12, management_enabled = $13, members_can_invite = $14,
            blossom_enabled = $15, livekit_enabled = $16, push_enabled = $17,
            custom_domain = $18,
            custom_domain_verified = COALESCE(
                custom_domain_verified AND custom_domain = $18 AND domain = $3 AND subdomain = $2,
                false
            ),
            policy_anonymous_read = $19,
            blossom_authenticated_read = $20, member_price = $21, member_interval = $22
         WHERE id = $1",
    )
    .bind(&relay.id)
    .bind(&relay.subdomain)
    .bind(&relay.domain)
    .bind(&relay.plan_id)
    .bind(&relay.info_name)
    .bind(&relay.info_icon)
    .bind(&relay.info_description)
    .bind(relay.policy_public_read)
    .bind(relay.policy_public_write)
    .bind(relay.policy_public_join)
    .bind(relay.policy_strip_signatures)
    .bind(relay.groups_enabled)
    .bind(relay.management_enabled)
    .bind(relay.members_can_invite)
    .bind(relay.blossom_enabled)
    .bind(relay.livekit_enabled)
    .bind(relay.push_enabled)
    .bind(&relay.custom_domain)
    .bind(relay.policy_anonymous_read)
    .bind(relay.blossom_authenticated_read)
    .bind(relay.member_price)
    .bind(relay.member_interval)
    .execute(conn)
    .await
    .map_err(|error| map_relay_write_error(error.into()))?;
    Ok(())
}

/// Change the name, description or icon the relay publishes, leaving `None`s as
/// they are.
pub async fn set_relay_info(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    name: Option<&str>,
    description: Option<&str>,
    icon: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE relay SET info_name = COALESCE($2, info_name), info_description = COALESCE($3, info_description),
            info_icon = COALESCE($4, info_icon)
         WHERE id = $1",
    )
    .bind(relay_id)
    .bind(name)
    .bind(description)
    .bind(icon)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_relay_status(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    status: RelayStatus,
) -> Result<()> {
    sqlx::query("UPDATE relay SET status = $1 WHERE id = $2")
        .bind(status)
        .bind(relay_id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn set_relay_purged_at(db: impl PgExecutor<'_>, relay_id: &str, at: i64) -> Result<()> {
    sqlx::query("UPDATE relay SET purged_at = $1 WHERE id = $2")
        .bind(at)
        .bind(relay_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Record whether DNS verification sees the relay's custom domain pointing at it,
/// but only if the custom domain is still `custom_domain` and no other live relay
/// has it verified. Returns whether it wrote.
pub async fn set_custom_domain_verified(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    custom_domain: &str,
    verified: bool,
) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE relay SET custom_domain_verified = $1
         WHERE id = $2 AND custom_domain = $3 AND purged_at IS NULL
           AND NOT ($1 AND EXISTS (
               SELECT 1 FROM relay other
               WHERE other.custom_domain = $3 AND other.custom_domain_verified
                 AND other.purged_at IS NULL AND other.id <> $2
           ))",
    )
    .bind(verified)
    .bind(relay_id)
    .bind(custom_domain)
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// A taken subdomain is the caller's to fix.
fn map_relay_write_error(error: Error) -> Error {
    let constraint = match &error {
        Error::Internal(inner) => inner
            .downcast_ref::<sqlx::Error>()
            .and_then(caravel_util::db::unique_violation)
            .map(str::to_string),
        _ => None,
    };

    match constraint.as_deref() {
        Some("relay_subdomain_key") => {
            Error::invalid("subdomain-exists", "subdomain already exists")
        }
        _ => error,
    }
}
