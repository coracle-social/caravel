use anyhow::Result;
use nostr_sdk::prelude::*;
use std::time::Duration;

use crate::command::Command;
use crate::env::Env;
use crate::models::{Activity, RELAY_STATUS_DELINQUENT, RELAY_STATUS_INACTIVE, Relay};
use crate::query::Query;

const RELAY_SYNC_RETRY_BASE_DELAY_SECS: u64 = 30;
const RELAY_SYNC_RETRY_MAX_DELAY_SECS: u64 = 15 * 60;
const RELAY_SYNC_RETRY_MAX_ATTEMPTS: usize = 6;

#[derive(Clone)]
pub struct Infra {
    env: Env,
    query: Query,
    command: Command,
}

impl Infra {
    pub fn new(query: Query, command: Command, env: &Env) -> Self {
        Self {
            env: env.clone(),
            query,
            command,
        }
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
        let needs_sync = matches!(
            activity.activity_type.as_str(),
            "create_relay" | "update_relay" | "activate_relay" | "deactivate_relay" | "fail_relay_sync"
        );

        if activity.resource_type != "relay" || !needs_sync {
            return Ok(());
        }

        if activity.activity_type == "fail_relay_sync" {
            self.schedule_relay_sync_retry(&activity.resource_id, "activity").await?;
            return Ok(());
        }

        let Some(relay) = self.query.get_relay(&activity.resource_id).await? else {
            return Ok(());
        };

        self.sync_relay(&relay).await;
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
                self.sync_relay(&relay).await;
            } else {
                self.schedule_relay_sync_retry(&relay.id, source).await?;
            }
        }

        Ok(())
    }

    async fn schedule_relay_sync_retry(&self, relay_id: &str, source: &str) -> Result<()> {
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

        let activities = self.query.list_activity_for_resource(relay_id).await?;
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
        let infra = self.clone();

        tokio::spawn(async move {
            tokio::time::sleep(delay).await;

            match infra.query.get_relay(&relay_id).await {
                Ok(Some(relay)) => infra.sync_relay(&relay).await,
                Ok(None) => {}
                Err(e) => {
                    tracing::error!(relay = %relay_id, error = %e, "relay sync retry task failed");
                }
            }
        });

        Ok(())
    }

    async fn sync_relay(&self, relay: &Relay) {
        match self.try_sync_relay(relay).await {
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

    async fn try_sync_relay(&self, relay: &Relay) -> Result<()> {
        // A relay is "new" (POST with a freshly generated secret) only if it has
        // never completed a sync. `synced == 1` short-circuits the activity lookup;
        // otherwise check the activity history so that a re-sync after an update
        // (which resets `synced` to 0) PATCHes instead of clobbering the secret.
        let is_new = relay.synced != 1
            && self
                .query
                .get_latest_activity_for_resource_and_type(&relay.id, "complete_relay_sync")
                .await?
                .is_none();

        let mut body = serde_json::json!({
            "host": format!("{}.{}", relay.subdomain, self.env.relay_domain),
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
            "blossom": if relay.blossom_enabled == 1 {
                serde_json::json!({
                    "enabled": true,
                    "adapter": "s3",
                    "s3": {
                        "endpoint": self.env.blossom_s3_endpoint,
                        "region": self.env.blossom_s3_region,
                        "bucket": self.env.blossom_s3_bucket,
                        "access_key": self.env.blossom_s3_access_key,
                        "secret_key": self.env.blossom_s3_secret_key,
                        "key_prefix": relay.schema,
                    },
                })
            } else {
                serde_json::json!({ "enabled": false })
            },
            "livekit": if relay.livekit_enabled == 1 {
                serde_json::json!({
                    "enabled": true,
                    "server_url": self.env.livekit_url,
                    "api_key": self.env.livekit_api_key,
                    "api_secret": self.env.livekit_api_secret,
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
        if is_new {
            if let Some(obj) = body.as_object_mut() {
                obj.insert(
                    "secret".to_string(),
                    serde_json::Value::String(Keys::generate().secret_key().to_secret_hex()),
                );
            }
        }

        let method = if is_new { HttpMethod::POST } else { HttpMethod::PATCH };
        self.request(method, &format!("relay/{}", relay.id), Some(&body))
            .await?;
        Ok(())
    }

    pub async fn list_relay_members(&self, relay_id: &str) -> Result<Vec<String>> {
        #[derive(serde::Deserialize)]
        struct MembersResponse {
            members: Vec<String>,
        }

        let response = self
            .request(HttpMethod::GET, &format!("relay/{relay_id}/members"), None)
            .await?;
        let parsed: MembersResponse = response.json().await?;
        Ok(parsed.members)
    }

    // Internal utilities

    /// Sends an authenticated request to the zooid API at `path` (relative to
    /// `env.zooid_api_url`). Returns the response on 2xx; bails with the body
    /// text otherwise.
    async fn request(
        &self,
        method: HttpMethod,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<reqwest::Response> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;
        let base = self.env.zooid_api_url.trim_end_matches('/');
        let path = path.trim_start_matches('/');
        let url = format!("{base}/{path}");
        let auth = self.env.make_auth(&url, method).await?;

        let reqwest_method = match method {
            HttpMethod::GET => reqwest::Method::GET,
            HttpMethod::POST => reqwest::Method::POST,
            HttpMethod::PUT => reqwest::Method::PUT,
            HttpMethod::PATCH => reqwest::Method::PATCH,
        };

        let mut req = client
            .request(reqwest_method, &url)
            .header("Authorization", auth);
        if let Some(body) = body {
            req = req.json(body);
        }

        let response = req.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("zooid {method} {path} returned {status}: {text}");
        }
        Ok(response)
    }
}

