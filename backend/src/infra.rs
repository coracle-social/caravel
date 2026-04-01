use anyhow::Result;
use nostr_sdk::prelude::*;
use tokio::sync::Mutex;

use crate::command::Command;
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
    last_activity_at: std::sync::Arc<Mutex<i64>>,
}

impl Infra {
    pub fn new(query: Query, command: Command) -> Self {
        let api_url = std::env::var("ZOOID_API_URL").unwrap_or_default();
        let relay_domain = std::env::var("RELAY_DOMAIN").unwrap_or_default();
        let livekit_url = std::env::var("LIVEKIT_URL").unwrap_or_default();
        let livekit_api_key = std::env::var("LIVEKIT_API_KEY").unwrap_or_default();
        let livekit_api_secret = std::env::var("LIVEKIT_API_SECRET").unwrap_or_default();
        let api_secret = std::env::var("ZOOID_API_SECRET").unwrap_or_default();
        Self {
            api_url,
            relay_domain,
            livekit_url,
            livekit_api_key,
            livekit_api_secret,
            api_secret,
            query,
            command,
            last_activity_at: std::sync::Arc::new(Mutex::new(0)),
        }
    }

    pub async fn start(self) {
        // Initialize from DB so we don't replay historical activities on restart
        match self.query.max_activity_at().await {
            Ok(ts) => *self.last_activity_at.lock().await = ts,
            Err(e) => tracing::error!(error = %e, "failed to read max activity timestamp"),
        }

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            interval.tick().await;
            if let Err(e) = self.tick().await {
                tracing::error!(error = %e, "infra tick failed");
            }
        }
    }

    pub async fn tick(&self) -> Result<()> {
        let mut since_guard = self.last_activity_at.lock().await;
        let since = *since_guard;
        let activity = self.query.list_activity(&since).await?;

        for a in activity {
            let needs_sync = matches!(
                a.activity_type.as_str(),
                "create_relay" | "update_relay" | "deactivate_relay"
            );

            if needs_sync {
                let Some(relay) = self.query.get_relay(&a.resource_id).await? else {
                    continue;
                };

                let is_new = relay.synced == 0;

                match self.sync_relay(&relay, is_new).await {
                    Ok(()) => {
                        tracing::info!(relay = %relay.id, "relay sync succeeded");
                        self.command.mark_relay_synced(&relay.id).await?
                    }
                    Err(e) => {
                        tracing::warn!(relay = %relay.id, error = %e, "relay sync failed");
                        self.command.fail_relay_sync(&relay, e.to_string()).await?;
                    }
                }
            }

            *since_guard = (*since_guard).max(a.created_at);
        }

        Ok(())
    }

    async fn nip98_auth(&self, url: &str, method: HttpMethod) -> Result<String> {
        let keys = Keys::parse(&self.api_secret)?;
        let server_url = Url::parse(url)?;
        let auth = HttpData::new(server_url, method).to_authorization(&keys).await?;
        Ok(auth)
    }

    async fn sync_relay(&self, relay: &crate::models::Relay, is_new: bool) -> Result<()> {
        let client = reqwest::Client::new();
        let base = self.api_url.trim_end_matches('/');

        let host = if self.relay_domain.is_empty() {
            relay.subdomain.clone()
        } else {
            format!("{}.{}", relay.subdomain, self.relay_domain)
        };

        let secret = Keys::generate().secret_key().to_secret_hex();

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

        let body = serde_json::json!({
            "host": host,
            "schema": relay.schema,
            "secret": secret,
            "inactive": relay.status == "inactive",
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

        let response = if is_new {
            let url = format!("{}/relay/{}", base, relay.id);
            let auth = self.nip98_auth(&url, HttpMethod::POST).await?;
            client.post(&url).header("Authorization", auth).json(&body).send().await?
        } else {
            let url = format!("{}/relay/{}", base, relay.id);
            let auth = self.nip98_auth(&url, HttpMethod::PUT).await?;
            client.put(&url).header("Authorization", auth).json(&body).send().await?
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("zooid sync returned {}: {}", status, body)
        }
        Ok(())
    }
}
