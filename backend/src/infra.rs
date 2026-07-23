//! The relay-provisioning reactor: it keeps the external relay backend (the
//! zooid API) in sync with our relay rows, reacting to relay activity and
//! retrying failed syncs with backoff.

use anyhow::Result;
use nostr_sdk::prelude::*;
use std::time::Duration;

use crate::command;
use crate::db;
use crate::domains;
use crate::env;
use crate::models::{Activity, RELAY_STATUS_DELINQUENT, RELAY_STATUS_INACTIVE, Relay};
use crate::query;

const RELAY_SYNC_RETRY_BASE_DELAY_SECS: u64 = 30;
const RELAY_SYNC_RETRY_MAX_DELAY_SECS: u64 = 15 * 60;
const RELAY_SYNC_RETRY_MAX_ATTEMPTS: usize = 6;

const DOMAIN_VERIFY_INTERVAL_SECS: u64 = 30;
const DOMAIN_REVERIFY_INTERVAL_SECS: u64 = 3 * 60 * 60;

/// Poll for relays with an unverified custom domain and attempt DNS verification.
/// Runs for the life of the process; each cycle waits for all checks to finish
/// before sleeping, so cycles never overlap.
pub async fn start_domain_verification() {
    loop {
        if let Err(e) = verify_pending_custom_domains().await {
            tracing::error!(error = %e, "domain verification poll failed");
        }
        tokio::time::sleep(Duration::from_secs(DOMAIN_VERIFY_INTERVAL_SECS)).await;
    }
}

async fn verify_pending_custom_domains() -> Result<()> {
    let relays = query::list_relays_with_unverified_custom_domain().await?;
    for relay in relays {
        let canonical = relay.canonical_host();
        match domains::domain_points_to(&relay.custom_domain, &canonical).await {
            Ok(true) => {
                tracing::info!(
                    relay = %relay.id,
                    domain = %relay.custom_domain,
                    "custom domain verified",
                );
                if let Err(e) = command::verify_relay_custom_domain(&relay.id).await {
                    tracing::error!(relay = %relay.id, error = %e, "failed to mark domain verified");
                    continue;
                }
                // Fetch the updated relay and sync so Zooid learns the new host.
                match query::get_relay(&relay.id).await {
                    Ok(Some(updated)) => sync_relay(&updated).await,
                    Ok(None) => {}
                    Err(e) => {
                        tracing::error!(relay = %relay.id, error = %e, "failed to fetch relay after domain verify")
                    }
                }
            }
            Ok(false) => {
                tracing::debug!(
                    relay = %relay.id,
                    domain = %relay.custom_domain,
                    target = %canonical,
                    "custom domain not yet pointing to relay",
                );
            }
            Err(e) => {
                tracing::warn!(relay = %relay.id, error = %e, "DNS check failed for custom domain");
            }
        }
    }
    Ok(())
}

/// Poll verified custom domains every 3 hours and un-verify any whose DNS no
/// longer points at the relay. Triggers a Zooid sync so the relay reverts to
/// its canonical subdomain as its host.
pub async fn start_domain_reverification() {
    loop {
        tokio::time::sleep(Duration::from_secs(DOMAIN_REVERIFY_INTERVAL_SECS)).await;
        if let Err(e) = check_verified_custom_domains().await {
            tracing::error!(error = %e, "verified domain check poll failed");
        }
    }
}

async fn check_verified_custom_domains() -> Result<()> {
    let relays = query::list_relays_with_verified_custom_domain().await?;
    for relay in relays {
        let canonical = relay.canonical_host();
        match domains::domain_points_to(&relay.custom_domain, &canonical).await {
            Ok(true) => {
                tracing::debug!(
                    relay = %relay.id,
                    domain = %relay.custom_domain,
                    "verified custom domain still points to relay",
                );
            }
            Ok(false) => {
                tracing::warn!(
                    relay = %relay.id,
                    domain = %relay.custom_domain,
                    "verified custom domain no longer points to relay; removing verification",
                );
                if let Err(e) = command::unverify_relay_custom_domain(&relay.id).await {
                    tracing::error!(relay = %relay.id, error = %e, "failed to un-verify custom domain");
                    continue;
                }
                match query::get_relay(&relay.id).await {
                    Ok(Some(updated)) => sync_relay(&updated).await,
                    Ok(None) => {}
                    Err(e) => {
                        tracing::error!(relay = %relay.id, error = %e, "failed to fetch relay after domain un-verify")
                    }
                }
            }
            Err(e) => {
                tracing::warn!(relay = %relay.id, error = %e, "DNS check failed for verified custom domain");
            }
        }
    }
    Ok(())
}

/// Run the reactor for the life of the process: reconcile any relays left
/// unsynced from a previous run, then sync each relay as its activity arrives.
pub async fn start() {
    let mut rx = db::subscribe();

    if let Err(error) = reconcile_relay_state("startup").await {
        tracing::error!(error = %error, "failed to reconcile relay state on startup");
    }

    loop {
        match rx.recv().await {
            Ok(activity) => {
                if let Err(e) = handle_activity(&activity).await {
                    tracing::error!(error = %e, "infra handle_activity failed");
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(missed = n, "infra lagged");

                if let Err(error) = reconcile_relay_state("lagged").await {
                    tracing::error!(error = %error, "failed to reconcile relay state after lag");
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn handle_activity(activity: &Activity) -> Result<()> {
    let needs_sync = matches!(
        activity.activity_type.as_str(),
        "create_relay"
            | "update_relay"
            | "activate_relay"
            | "deactivate_relay"
            | "purge_relay"
            | "fail_relay_sync"
    );

    if activity.resource_type != "relay" || !needs_sync {
        return Ok(());
    }

    if activity.activity_type == "fail_relay_sync" {
        schedule_relay_sync_retry(&activity.resource_id, "activity").await?;
        return Ok(());
    }

    let Some(relay) = query::get_relay(&activity.resource_id).await? else {
        return Ok(());
    };

    sync_relay(&relay).await;
    Ok(())
}

async fn reconcile_relay_state(source: &str) -> Result<()> {
    let relays = query::list_relays_pending_sync().await?;

    if relays.is_empty() {
        return Ok(());
    }

    tracing::info!(
        source,
        relay_count = relays.len(),
        "reconciling pending relay state"
    );

    for relay in relays {
        if relay.sync_error.trim().is_empty() {
            sync_relay(&relay).await;
        } else {
            schedule_relay_sync_retry(&relay.id, source).await?;
        }
    }

    Ok(())
}

async fn schedule_relay_sync_retry(relay_id: &str, source: &str) -> Result<()> {
    fn get_retry_delay(consecutive_failures: usize) -> Option<Duration> {
        let retry_attempt = consecutive_failures.max(1);
        if retry_attempt > RELAY_SYNC_RETRY_MAX_ATTEMPTS {
            return None;
        }

        let exponent = (retry_attempt - 1).min(31);
        let multiplier = 1u64 << exponent;
        let delay_secs = RELAY_SYNC_RETRY_BASE_DELAY_SECS
            .saturating_mul(multiplier)
            .min(RELAY_SYNC_RETRY_MAX_DELAY_SECS);

        Some(Duration::from_secs(delay_secs))
    }

    let activities = query::list_activity_for_resource(relay_id).await?;
    let consecutive_failures = activities
        .iter()
        .take_while(|activity| activity.activity_type == "fail_relay_sync")
        .count();

    let Some(delay) = get_retry_delay(consecutive_failures) else {
        tracing::warn!(
            relay = relay_id,
            consecutive_failures,
            max_attempts = RELAY_SYNC_RETRY_MAX_ATTEMPTS,
            "relay sync retries exhausted; awaiting manual intervention"
        );
        return Ok(());
    };

    tracing::info!(
        relay = relay_id,
        source,
        consecutive_failures,
        delay_secs = delay.as_secs(),
        "scheduled relay sync retry"
    );

    let relay_id = relay_id.to_string();

    tokio::spawn(async move {
        tokio::time::sleep(delay).await;

        match query::get_relay(&relay_id).await {
            Ok(Some(relay)) => sync_relay(&relay).await,
            Ok(None) => {}
            Err(e) => {
                tracing::error!(relay = %relay_id, error = %e, "relay sync retry task failed");
            }
        }
    });

    Ok(())
}

async fn sync_relay(relay: &Relay) {
    // A purged relay is never re-provisioned — pending work for it is the
    // purge itself (e.g. a retry after a failed delete).
    if relay.purged_at.is_some() {
        return purge_relay(relay).await;
    }

    match try_sync_relay(relay).await {
        Ok(()) => {
            tracing::info!(relay = %relay.id, "relay sync succeeded");
            if let Err(e) = command::complete_relay_sync(relay).await {
                tracing::error!(relay = %relay.id, error = %e, "failed to mark sync complete");
            }
        }
        Err(e) => {
            tracing::warn!(relay = %relay.id, error = %e, "relay sync failed");
            if let Err(e2) = command::fail_relay_sync(relay, e.to_string()).await {
                tracing::error!(relay = %relay.id, error = %e2, "failed to record sync failure");
            }
        }
    }
}

async fn try_sync_relay(relay: &Relay) -> Result<()> {
    // A relay is "new" (POST with a freshly generated secret) only if it has
    // never completed a sync. `synced == 1` short-circuits the activity lookup;
    // otherwise check the activity history so that a re-sync after an update
    // (which resets `synced` to 0) PATCHes instead of clobbering the secret.
    let is_new = relay.synced != 1
        && query::get_latest_activity_for_resource_and_type(&relay.id, "complete_relay_sync")
            .await?
            .is_none();

    let host = relay.host();

    let mut redirect_from: serde_json::Map<String, serde_json::Value> = relay
        .redirect_from
        .0
        .iter()
        .map(|from| (from.clone(), serde_json::json!({ "status": 302 })))
        .collect();

    // While a verified custom domain is the effective host, zooid no longer
    // serves the canonical <subdomain>.<zooid_domain> host directly, so redirect
    // it to the custom domain too. Derived at sync time rather than stored so it
    // stays correct across subdomain changes and custom-domain un-verification.
    let canonical = relay.canonical_host();
    if canonical != host {
        redirect_from.insert(
            canonical,
            serde_json::json!({ "status": 302 }),
        );
    }

    let mut body = serde_json::json!({
        "host": host,
        "schema": relay.id,
        "redirect_from": redirect_from,
        "inactive": relay.status == RELAY_STATUS_INACTIVE
            || relay.status == RELAY_STATUS_DELINQUENT,
        "info": {
            "name": relay.info_name,
            "icon": relay.info_icon,
            "description": relay.info_description,
            "pubkey": relay.tenant_pubkey,
        },
        "policy": {
            "public_read": relay.policy_public_read == 1,
            "public_write": relay.policy_public_write == 1,
            "public_join": relay.policy_public_join == 1,
            "strip_signatures": relay.policy_strip_signatures == 1,
        },
        "groups": { "enabled": relay.groups_enabled == 1 },
        "management": { "enabled": relay.management_enabled == 1 },
        "blossom": if relay.blossom_enabled == 1 {
            serde_json::json!({
                "enabled": true,
                "adapter": "s3",
                "s3": {
                    "endpoint": env::get().blossom_s3_endpoint,
                    "region": env::get().blossom_s3_region,
                    "bucket": env::get().blossom_s3_bucket,
                    "access_key": env::get().blossom_s3_access_key,
                    "secret_key": env::get().blossom_s3_secret_key,
                    "key_prefix": relay.id,
                },
            })
        } else {
            serde_json::json!({ "enabled": false })
        },
        "livekit": if relay.livekit_enabled == 1 {
            serde_json::json!({
                "enabled": true,
                "server_url": env::get().livekit_url,
                "api_key": env::get().livekit_api_key,
                "api_secret": env::get().livekit_api_secret,
            })
        } else {
            serde_json::json!({ "enabled": false })
        },
        "push": { "enabled": relay.push_enabled == 1 },
        "roles": {
            "admin": { "can_manage": true, "can_invite": true },
            "member": { "can_invite": true },
        },
    });

    // Only provide a secret if the relay is new. This allows us to not store the relay secrets on our side.
    if is_new && let Some(obj) = body.as_object_mut() {
        obj.insert(
            "secret".to_string(),
            serde_json::Value::String(Keys::generate().secret_key().to_secret_hex()),
        );
    }

    let method = if is_new {
        reqwest::Method::POST
    } else {
        reqwest::Method::PATCH
    };
    request(method, &format!("relay/{}", relay.id), Some(&body)).await?;
    Ok(())
}

/// Delete a purged relay's data and config from zooid. On failure the error is
/// recorded via `fail_relay_sync`, which schedules a retry with the usual
/// backoff — the purge is only complete once zooid confirms it.
async fn purge_relay(relay: &Relay) {
    match try_purge_relay(relay).await {
        Ok(()) => {
            tracing::info!(relay = %relay.id, "relay purge succeeded");
        }
        Err(e) => {
            tracing::warn!(relay = %relay.id, error = %e, "relay purge failed");
            if let Err(e2) = command::fail_relay_sync(relay, e.to_string()).await {
                tracing::error!(relay = %relay.id, error = %e2, "failed to record purge failure");
            }
        }
    }
}

/// A 404 counts as success: the relay is already gone from zooid, which is
/// exactly the state the purge is trying to reach.
async fn try_purge_relay(relay: &Relay) -> Result<()> {
    let path = format!("relay/{}", relay.id);
    let response = send(&reqwest::Method::DELETE, &path, None).await?;

    if response.status() == reqwest::StatusCode::NOT_FOUND || response.status().is_success() {
        return Ok(());
    }

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    anyhow::bail!("zooid DELETE {path} returned {status}: {text}");
}

/// Fetch the member pubkeys of a relay from the zooid API.
pub async fn list_relay_members(relay_id: &str) -> Result<Vec<String>> {
    #[derive(serde::Deserialize)]
    struct MembersResponse {
        members: Vec<String>,
    }

    let response = request(
        reqwest::Method::GET,
        &format!("relay/{relay_id}/members"),
        None,
    )
    .await?;
    let parsed: MembersResponse = response.json().await?;
    Ok(parsed.members)
}

/// One reviewable item from a relay: a published event or an uploaded blob,
/// tagged with the relay (zooid tenant schema, which is our relay id) it came
/// from. `data` is the raw nostr event for `"event"` items and a blob
/// descriptor for `"upload"` items.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ModerationItem {
    #[serde(rename = "type")]
    pub item_type: String,
    pub tenant: String,
    pub data: serde_json::Value,
}

/// One page of the moderation feed. `truncated` reports that the requested
/// window held more than fit, so the caller has ground left to cover inside it.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ModerationPage {
    pub items: Vec<ModerationItem>,
    pub truncated: bool,
}

/// Fetch the newest events and uploads across every relay within a window from
/// the zooid API. Both bounds are optional inclusive unix timestamps.
pub async fn list_moderation(since: Option<i64>, until: Option<i64>) -> Result<ModerationPage> {
    let bounds: Vec<String> = [("since", since), ("until", until)]
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| format!("{key}={value}")))
        .collect();

    let path = if bounds.is_empty() {
        "moderation".to_string()
    } else {
        format!("moderation?{}", bounds.join("&"))
    };

    let response = request(reqwest::Method::GET, &path, None).await?;
    Ok(response.json().await?)
}

/// Fetch one stored blob from the zooid API, returning its content type and
/// bytes. Goes through zooid's whitelisted management API rather than the
/// relay's public Blossom URL, which gates reads on relay membership that we
/// don't hold. Subject to the same 5s client timeout as every other zooid call,
/// against a blob that can be up to 10 MiB.
/// `None` when zooid doesn't host that relay or never stored that blob, which
/// is an ordinary miss rather than a failure — zooid, not our relay table, is
/// the authority here, since it also serves relays we never provisioned.
pub async fn get_relay_blob(relay_id: &str, sha256: &str) -> Result<Option<(String, Vec<u8>)>> {
    let path = format!("relay/{relay_id}/blob/{sha256}");
    let response = send(&reqwest::Method::GET, &path, None).await?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("zooid GET {path} returned {status}: {text}");
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    Ok(Some((content_type, response.bytes().await?.to_vec())))
}

/// Sends an authenticated request to the zooid API at `path` (relative to
/// `env.zooid_api_url`). Returns the response on 2xx; bails with the body
/// text otherwise.
async fn request(
    method: reqwest::Method,
    path: &str,
    body: Option<&serde_json::Value>,
) -> Result<reqwest::Response> {
    let response = send(&method, path, body).await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("zooid {method} {path} returned {status}: {text}");
    }

    Ok(response)
}

/// The bare authenticated call, without the 2xx check, for callers that read a
/// status as data rather than as a failure.
async fn send(
    method: &reqwest::Method,
    path: &str,
    body: Option<&serde_json::Value>,
) -> Result<reqwest::Response> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let base = env::get().zooid_api_url.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    let url = format!("{base}/{path}");
    // Zooid matches the NIP-98 `u` tag against the request path alone, so a
    // signature over a url carrying a query string is rejected.
    let auth_url = url.split('?').next().unwrap_or(&url);
    let auth = env::get().make_auth(auth_url, method.as_str()).await?;

    let mut req = client
        .request(method.clone(), &url)
        .header("Authorization", auth);
    if let Some(body) = body {
        req = req.json(body);
    }

    Ok(req.send().await?)
}
