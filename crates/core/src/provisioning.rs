//! Keeps zooid, which serves the relays until the relay runtime moves into this
//! binary, holding each relay's current config.

use serde_json::json;

use crate::Core;
use crate::error::Result;
use crate::relays::{self, Relay, RelayStatus};

/// Give zooid a relay's current config, or delete a purged relay from it, and
/// record the outcome on the relay.
pub async fn sync_relay(core: &Core, relay_id: &str) -> Result<()> {
    let Some(relay) = relays::get_relay(core.db.pool(), relay_id).await? else {
        return Ok(());
    };

    let result = async {
        if relay.is_purged() {
            return core.zooid.delete_relay(&relay.id).await;
        }
        core.zooid
            .put_relay(&relay.id, &zooid_config(core, &relay)?)
            .await
    }
    .await;

    match result {
        Ok(()) => {
            relays::complete_relay_sync(core.db.pool(), &relay.id).await?;
            tracing::info!(relay = %relay.id, purged = relay.is_purged(), "relay synced to zooid");
        }
        Err(error) => {
            tracing::warn!(relay = %relay.id, error = %error, "relay sync failed");
            relays::fail_relay_sync(core.db.pool(), &relay.id, &error.to_string()).await?;
        }
    }

    Ok(())
}

/// Sync every relay zooid doesn't hold the current state of.
pub async fn sync_pending_relays(core: &Core) -> Result<()> {
    for relay in relays::list_relays_pending_sync(core.db.pool()).await? {
        sync_relay(core, &relay.id).await?;
    }
    Ok(())
}

fn zooid_config(core: &Core, relay: &Relay) -> anyhow::Result<serde_json::Value> {
    let config = &core.config;
    let host = relay.host();

    // While a verified custom domain is the relay's host, its canonical host
    // redirects there too.
    let mut redirect_from: serde_json::Map<String, serde_json::Value> = relay
        .redirect_from
        .iter()
        .map(|from| (from.clone(), json!({ "status": 302 })))
        .collect();
    if relay.canonical_host() != host {
        redirect_from.insert(relay.canonical_host(), json!({ "status": 302 }));
    }

    Ok(json!({
        "host": host,
        "schema": relay.id,
        "secret": config.platform_key.decrypt(&relay.secret)?,
        "redirect_from": redirect_from,
        "inactive": relay.status != RelayStatus::Active,
        "unmoderated": config.is_admin(&relay.tenant_pubkey),
        "info": {
            "name": relay.info_name,
            "icon": relay.info_icon,
            "description": relay.info_description,
            "pubkey": relay.tenant_pubkey,
        },
        "policy": {
            "public_read": relay.policy_public_read,
            "public_write": relay.policy_public_write,
            "public_join": relay.policy_public_join,
            "strip_signatures": relay.policy_strip_signatures,
        },
        "groups": { "enabled": relay.groups_enabled },
        "management": {
            "enabled": relay.management_enabled,
            "member_methods": if relay.members_can_invite { vec!["listclaims", "createclaim"] } else { vec![] },
        },
        "blossom": if relay.blossom_enabled {
            json!({
                "enabled": true,
                "adapter": "s3",
                "s3": {
                    "endpoint": config.blossom_s3.endpoint,
                    "region": config.blossom_s3.region,
                    "bucket": config.blossom_s3.bucket,
                    "access_key": config.blossom_s3.access_key,
                    "secret_key": config.blossom_s3.secret_key,
                    "key_prefix": relay.id,
                },
            })
        } else {
            json!({ "enabled": false })
        },
        "livekit": if relay.livekit_enabled {
            json!({
                "enabled": true,
                "server_url": config.livekit.url,
                "api_key": config.livekit.api_key,
                "api_secret": config.livekit.api_secret,
            })
        } else {
            json!({ "enabled": false })
        },
        "push": { "enabled": relay.push_enabled },
    }))
}
