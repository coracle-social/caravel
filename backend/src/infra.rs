use anyhow::Result;
use nostr_sdk::prelude::*;
use std::time::Duration;

use crate::command::Command;
use crate::models::{Activity, RELAY_STATUS_DELINQUENT, RELAY_STATUS_INACTIVE, Relay};
use crate::query::Query;

const RELAY_SYNC_RETRY_BASE_DELAY_SECS: u64 = 30;
const RELAY_SYNC_RETRY_MAX_DELAY_SECS: u64 = 15 * 60;
const RELAY_SYNC_RETRY_MAX_ATTEMPTS: usize = 6;

/// Blossom S3 settings from env; relay sync sets `key_prefix` to the relay schema.
#[derive(Clone)]
struct BlossomS3Sync {
    endpoint: String,
    region: String,
    bucket: String,
    access_key: String,
    secret_key: String,
}

impl BlossomS3Sync {
    fn from_env() -> Option<Self> {
        let region = std::env::var("BLOSSOM_S3_REGION").unwrap_or_default();
        let bucket = std::env::var("BLOSSOM_S3_BUCKET").unwrap_or_default();
        let access_key = std::env::var("BLOSSOM_S3_ACCESS_KEY").unwrap_or_default();
        let secret_key = std::env::var("BLOSSOM_S3_SECRET_KEY").unwrap_or_default();

        let region = region.trim().to_string();
        let bucket = bucket.trim().to_string();
        let access_key = access_key.trim().to_string();
        let secret_key = secret_key.trim().to_string();

        if region.is_empty() || bucket.is_empty() || access_key.is_empty() || secret_key.is_empty() {
            return None;
        }

        let endpoint = std::env::var("BLOSSOM_S3_ENDPOINT")
            .unwrap_or_default()
            .trim()
            .to_string();

        Some(Self {
            endpoint,
            region,
            bucket,
            access_key,
            secret_key,
        })
    }
}

#[derive(Clone)]
pub struct Infra {
    api_url: String,
    relay_domain: String,
    livekit_url: String,
    livekit_api_key: String,
    livekit_api_secret: String,
    api_secret: String,
    blossom_s3: Option<BlossomS3Sync>,
    query: Query,
    command: Command,
}

impl Infra {
    pub fn new(query: Query, command: Command) -> Result<Self> {
        let api_url = std::env::var("ZOOID_API_URL").unwrap_or_default();
        let relay_domain = std::env::var("RELAY_DOMAIN").unwrap_or_default();
        let livekit_url = std::env::var("LIVEKIT_URL").unwrap_or_default();
        let livekit_api_key = std::env::var("LIVEKIT_API_KEY").unwrap_or_default();
        let livekit_api_secret = std::env::var("LIVEKIT_API_SECRET").unwrap_or_default();
        let api_secret = std::env::var("ZOOID_API_SECRET").unwrap_or_default();
        let blossom_s3 = BlossomS3Sync::from_env();

        if api_url.trim().is_empty() {
            anyhow::bail!("missing ZOOID_API_URL");
        }
        if api_secret.trim().is_empty() {
            anyhow::bail!("missing ZOOID_API_SECRET");
        }

        Ok(Self {
            api_url,
            relay_domain,
            livekit_url,
            livekit_api_key,
            livekit_api_secret,
            api_secret,
            blossom_s3,
            query,
            command,
        })
    }

    pub async fn start(self) {
        let mut rx = self.command.notify.subscribe();

        if let Err(error) = self.reconcile_relay_state("startup").await {
            tracing::error!(error = %error, "failed to reconcile relay state on startup");
        }

        loop {
            match rx.recv().await {
                Ok(activity) => {
                    if let Err(e) = self.handle_activity(&activity).await {
                        tracing::error!(error = %e, "infra handle_activity failed");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(missed = n, "infra lagged");

                    if let Err(error) = self.reconcile_relay_state("lagged").await {
                        tracing::error!(error = %error, "failed to reconcile relay state after lag");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    async fn handle_activity(&self, activity: &Activity) -> Result<()> {
        let needs_sync = should_sync_relay_activity(activity.activity_type.as_str());

        if !needs_sync || activity.resource_type != "relay" {
            return Ok(());
        }

        if activity.activity_type == "fail_relay_sync" {
            self.schedule_relay_sync_retry(&activity.resource_id, "activity")
                .await?;
            return Ok(());
        }

        let Some(relay) = self.query.get_relay(&activity.resource_id).await? else {
            return Ok(());
        };

        let is_new = self.relay_sync_is_new(&relay).await?;
        self.sync_and_report(&relay, is_new).await;

        Ok(())
    }

    async fn reconcile_relay_state(&self, source: &str) -> Result<()> {
        let relays = self.query.list_relays_pending_sync().await?;

        if relays.is_empty() {
            return Ok(());
        }

        tracing::info!(source, relay_count = relays.len(), "reconciling pending relay state");

        for relay in relays {
            if relay.sync_error.trim().is_empty() {
                let is_new = self.relay_sync_is_new(&relay).await?;
                self.sync_and_report(&relay, is_new).await;
            } else {
                self.schedule_relay_sync_retry(&relay.id, source).await?;
            }
        }

        Ok(())
    }

    async fn schedule_relay_sync_retry(&self, relay_id: &str, source: &str) -> Result<()> {
        let activities = self.query.list_activity_for_relay(relay_id).await?;
        let consecutive_failures = consecutive_sync_failures(&activities);

        let Some(delay) = relay_sync_retry_delay(consecutive_failures) else {
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
        let infra = self.clone();

        tokio::spawn(async move {
            tokio::time::sleep(delay).await;

            if let Err(e) = infra.retry_relay_sync(&relay_id).await {
                tracing::error!(relay = %relay_id, error = %e, "relay sync retry task failed");
            }
        });

        Ok(())
    }

    async fn retry_relay_sync(&self, relay_id: &str) -> Result<()> {
        let Some(relay) = self.query.get_relay(relay_id).await? else {
            return Ok(());
        };

        if relay.sync_error.trim().is_empty() {
            tracing::debug!(relay = %relay.id, "skip relay sync retry; relay has no sync_error");
            return Ok(());
        }

        let is_new = self.relay_sync_is_new(&relay).await?;
        self.sync_and_report(&relay, is_new).await;
        Ok(())
    }

    async fn relay_sync_is_new(&self, relay: &Relay) -> Result<bool> {
        if relay.synced == 1 {
            return Ok(false);
        }

        let has_completed_sync = self.query.relay_has_completed_sync(&relay.id).await?;
        Ok(!has_completed_sync)
    }

    async fn sync_and_report(&self, relay: &Relay, is_new: bool) {
        match self.sync_relay(relay, is_new).await {
            Ok(()) => {
                tracing::info!(relay = %relay.id, "relay sync succeeded");
                if let Err(e) = self.command.complete_relay_sync(&relay.id).await {
                    tracing::error!(relay = %relay.id, error = %e, "failed to mark sync complete");
                }
            }
            Err(e) => {
                tracing::warn!(relay = %relay.id, error = %e, "relay sync failed");
                if let Err(e2) = self.command.fail_relay_sync(relay, e.to_string()).await {
                    tracing::error!(relay = %relay.id, error = %e2, "failed to record sync failure");
                }
            }
        }
    }

    async fn nip98_auth(&self, url: &str, method: HttpMethod) -> Result<String> {
        let keys = Keys::parse(&self.api_secret)?;
        let server_url = Url::parse(url)?;
        let auth = HttpData::new(server_url, method)
            .to_authorization(&keys)
            .await?;
        Ok(auth)
    }

    pub async fn list_relay_members(&self, relay_id: &str) -> Result<Vec<String>> {
        let client = reqwest::Client::new();
        let base = self.api_url.trim_end_matches('/');
        let url = format!("{base}/relay/{relay_id}/members");
        let auth = self.nip98_auth(&url, HttpMethod::GET).await?;

        let response = client
            .get(&url)
            .header("Authorization", auth)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("zooid members returned {status}: {body}");
        }

        let body = response.text().await?;
        parse_relay_members_response(&body)
    }

    async fn sync_relay(&self, relay: &Relay, is_new: bool) -> Result<()> {
        let client = reqwest::Client::new();
        let base = self.api_url.trim_end_matches('/');

        let host = if self.relay_domain.is_empty() {
            relay.subdomain.clone()
        } else {
            format!("{}.{}", relay.subdomain, self.relay_domain)
        };

        let livekit = if relay.livekit_enabled == 1 {
            serde_json::json!({
                "enabled": true,
                "server_url": self.livekit_url,
                "api_key": self.livekit_api_key,
                "api_secret": self.livekit_api_secret,
            })
        } else {
            serde_json::json!({ "enabled": false })
        };

        let body = relay_sync_body(
            relay,
            host,
            livekit,
            is_new.then(|| Keys::generate().secret_key().to_secret_hex()),
            self.blossom_s3.as_ref(),
        );

        let url = format!("{}/relay/{}", base, relay.id);
        let auth = self
            .nip98_auth(&url, zooid_sync_http_method(is_new))
            .await?;

        let request = if is_new {
            client.post(&url)
        } else {
            client.patch(&url)
        };

        let response = request
            .header("Authorization", auth)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("zooid sync returned {status}: {body}")
        }
        Ok(())
    }
}

fn zooid_sync_http_method(is_new: bool) -> HttpMethod {
    if is_new {
        HttpMethod::POST
    } else {
        HttpMethod::PATCH
    }
}

fn parse_relay_members_response(body: &str) -> Result<Vec<String>> {
    let value: serde_json::Value = serde_json::from_str(body)?;

    if let Some(members) = members_from_value(&value) {
        return Ok(members);
    }
    if let Some(members) = value.get("members").and_then(members_from_value) {
        return Ok(members);
    }
    if let Some(members) = value
        .get("data")
        .and_then(|data| data.get("members"))
        .and_then(members_from_value)
    {
        return Ok(members);
    }

    anyhow::bail!("zooid members response missing members array")
}

fn members_from_value(value: &serde_json::Value) -> Option<Vec<String>> {
    let values = value.as_array()?;
    values
        .iter()
        .map(|value| value.as_str().map(ToString::to_string))
        .collect()
}

fn relay_sync_body(
    relay: &Relay,
    host: String,
    livekit: serde_json::Value,
    secret: Option<String>,
    blossom_s3: Option<&BlossomS3Sync>,
) -> serde_json::Value {
    let blossom = blossom_sync_json(relay, blossom_s3);

    let mut body = serde_json::json!({
        "host": host,
        "schema": relay.schema,
        "inactive": relay.status == RELAY_STATUS_INACTIVE
            || relay.status == RELAY_STATUS_DELINQUENT,
        "info": {
            "name": relay.info_name,
            "icon": relay.info_icon,
            "description": relay.info_description,
            "pubkey": relay.tenant,
        },
        "policy": {
            "public_join": relay.policy_public_join == 1,
            "strip_signatures": relay.policy_strip_signatures == 1,
        },
        "groups": { "enabled": relay.groups_enabled == 1 },
        "management": { "enabled": relay.management_enabled == 1 },
        "blossom": blossom,
        "livekit": livekit,
        "push": { "enabled": relay.push_enabled == 1 },
        "roles": {
            "admin": { "can_manage": true, "can_invite": true },
            "member": { "can_invite": true },
        },
    });

    if let (Some(secret), Some(body_obj)) = (secret, body.as_object_mut()) {
        body_obj.insert("secret".to_string(), serde_json::Value::String(secret));
    }

    body
}

fn blossom_sync_json(relay: &Relay, blossom_s3: Option<&BlossomS3Sync>) -> serde_json::Value {
    let enabled = relay.blossom_enabled == 1;
    if !enabled {
        return serde_json::json!({ "enabled": false });
    }

    let Some(s3) = blossom_s3 else {
        return serde_json::json!({ "enabled": true });
    };

    let mut s3_obj = serde_json::Map::new();
    if !s3.endpoint.trim().is_empty() {
        s3_obj.insert(
            "endpoint".to_string(),
            serde_json::Value::String(s3.endpoint.clone()),
        );
    }
    s3_obj.insert(
        "region".to_string(),
        serde_json::Value::String(s3.region.clone()),
    );
    s3_obj.insert(
        "bucket".to_string(),
        serde_json::Value::String(s3.bucket.clone()),
    );
    s3_obj.insert(
        "access_key".to_string(),
        serde_json::Value::String(s3.access_key.clone()),
    );
    s3_obj.insert(
        "secret_key".to_string(),
        serde_json::Value::String(s3.secret_key.clone()),
    );
    s3_obj.insert(
        "key_prefix".to_string(),
        serde_json::Value::String(relay.schema.clone()),
    );

    serde_json::json!({
        "enabled": true,
        "adapter": "s3",
        "s3": serde_json::Value::Object(s3_obj),
    })
}

fn should_sync_relay_activity(activity_type: &str) -> bool {
    matches!(
        activity_type,
        "create_relay" | "update_relay" | "activate_relay" | "deactivate_relay" | "fail_relay_sync"
    )
}

fn consecutive_sync_failures(activities: &[Activity]) -> usize {
    activities
        .iter()
        .take_while(|activity| activity.activity_type == "fail_relay_sync")
        .count()
}

fn relay_sync_retry_delay(consecutive_failures: usize) -> Option<Duration> {
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
