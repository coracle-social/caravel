use anyhow::Result;
use nostr_sdk::prelude::*;

use crate::command::Command;
use crate::models::{Activity, RELAY_STATUS_DELINQUENT, RELAY_STATUS_INACTIVE, Relay};
use crate::query::Query;

#[derive(Clone)]
pub struct Infra {
    api_url: String,
    relay_domain: String,
    livekit_url: String,
    livekit_api_key: String,
    livekit_api_secret: String,
    api_secret: String,
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
            query,
            command,
        })
    }

    pub async fn start(self) {
        let mut rx = self.command.notify.subscribe();

        loop {
            match rx.recv().await {
                Ok(activity) => {
                    if let Err(e) = self.handle_activity(&activity).await {
                        tracing::error!(error = %e, "infra handle_activity failed");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(missed = n, "infra lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    async fn handle_activity(&self, activity: &Activity) -> Result<()> {
        let needs_sync = should_sync_relay_activity(activity.activity_type.as_str());

        if needs_sync {
            let Some(relay) = self.query.get_relay(&activity.resource_id).await? else {
                return Ok(());
            };

            let is_new = relay.synced == 0;
            self.sync_and_report(&relay, is_new).await;
        }

        Ok(())
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
) -> serde_json::Value {
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
        "blossom": { "enabled": relay.blossom_enabled == 1 },
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

fn should_sync_relay_activity(activity_type: &str) -> bool {
    matches!(
        activity_type,
        "create_relay" | "update_relay" | "activate_relay" | "deactivate_relay"
    )
}
