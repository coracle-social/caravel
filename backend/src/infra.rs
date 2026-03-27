use anyhow::Result;
use tokio::sync::Mutex;

use crate::repo::Repo;

#[derive(Clone)]
pub struct Infra {
    api_url: String,
    relay_domain: String,
    livekit_url: String,
    livekit_api_key: String,
    livekit_api_secret: String,
    repo: Repo,
    last_activity_at: std::sync::Arc<Mutex<i64>>,
}

impl Infra {
    pub fn new(repo: Repo) -> Self {
        let api_url = std::env::var("ZOOID_API_URL").unwrap_or_default();
        let relay_domain = std::env::var("RELAY_DOMAIN").unwrap_or_default();
        let livekit_url = std::env::var("LIVEKIT_URL").unwrap_or_default();
        let livekit_api_key = std::env::var("LIVEKIT_API_KEY").unwrap_or_default();
        let livekit_api_secret = std::env::var("LIVEKIT_API_SECRET").unwrap_or_default();
        Self {
            api_url,
            relay_domain,
            livekit_url,
            livekit_api_key,
            livekit_api_secret,
            repo,
            last_activity_at: std::sync::Arc::new(Mutex::new(0)),
        }
    }

    pub async fn start(self) {
        // Initialize from DB so we don't replay historical activities on restart
        match self.repo.max_activity_at().await {
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
        let activity = self.repo.list_activity(&since).await?;

        for a in activity {
            let sync_type = match a.activity_type.as_str() {
                "create_relay" => Some(true),
                "update_relay" | "deactivate_relay" => Some(false),
                _ => None,
            };

            if let Some(is_new) = sync_type {
                let Some(relay) = self.repo.get_relay(&a.resource_id).await? else {
                    continue;
                };

                if let Err(e) = self.sync_relay(&relay, is_new).await {
                    tracing::warn!(relay = %relay.id, error = %e, "relay sync failed");
                    self.repo.fail_relay_sync(&relay, e.to_string()).await?;
                }
            }

            *since_guard = (*since_guard).max(a.created_at);
        }

        Ok(())
    }

    async fn sync_relay(&self, relay: &crate::models::Relay, is_new: bool) -> Result<()> {
        let client = reqwest::Client::new();
        let base = self.api_url.trim_end_matches('/');

        let host = if self.relay_domain.is_empty() {
            relay.subdomain.clone()
        } else {
            format!("{}.{}", relay.subdomain, self.relay_domain)
        };

        let secret = uuid::Uuid::new_v4().to_string();

        let livekit = if relay.livekit_enabled == 1 {
            serde_json::json!({
                "enabled": true,
                "url": self.livekit_url,
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
            "roles": [
                { "name": "admin", "permissions": ["read", "write", "admin"] },
                { "name": "member", "permissions": ["read", "write"] },
                { "name": "guest", "permissions": ["read"] },
            ],
        });

        let response = if is_new {
            client.post(format!("{}/relay", base)).json(&body).send().await?
        } else {
            client.put(format!("{}/relay/{}", base, relay.id)).json(&body).send().await?
        };

        if !response.status().is_success() {
            anyhow::bail!("zooid sync returned {}", response.status())
        }
        Ok(())
    }
}
