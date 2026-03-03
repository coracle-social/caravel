use anyhow::{Result, anyhow};
use rand::RngCore;
use rand::rngs::OsRng;
use reqwest::Client;
use serde_json::{Value, json};

use nostr_sdk::nostr::Keys;
use nostr_sdk::nostr::nips::nip98::{HttpData, HttpMethod};
use nostr_sdk::nostr::types::url::Url;

use crate::models::{Relay, RelayConfig};

#[derive(Clone)]
pub struct Provisioner {
    base_url: String,
    relay_domain: String,
    admin_keys: Keys,
    client: Client,
}

impl Provisioner {
    pub fn new(base_url: String, relay_domain: String, admin_secret: String) -> Result<Self> {
        if admin_secret.trim().is_empty() {
            return Err(anyhow!("PLATFORM_SECRET is required"));
        }

        let admin_keys = Keys::parse(&admin_secret)?;
        let client = Client::new();

        Ok(Self {
            base_url,
            relay_domain,
            admin_keys,
            client,
        })
    }

    /// Create a relay in zooid.
    ///
    /// POSTs the full config (including a generated secret and host).
    pub async fn create_relay(&self, relay: &Relay) -> Result<()> {
        let url = format!("{}/relay/{}", self.base_url.trim_end_matches('/'), relay.id);

        let blossom_default = relay.plan != "free";
        let livekit_default = relay.plan != "free";
        let cfg = relay.config.as_ref();
        let host = format!("{}.{}", relay.subdomain, self.relay_domain);
        let secret = generate_secret_hex();
        let payload = json!({
            "host": host,
            "schema": relay.id,
            "secret": secret,
            "info": {
                "name": relay.name,
                "icon": relay.icon,
                "pubkey": relay.tenant,
                "description": relay.description,
            },
            "policy": {
                "public_join": cfg_bool(cfg, |c| &c.policy, "public_join", false),
                "strip_signatures": cfg_bool(cfg, |c| &c.policy, "strip_signatures", false),
            },
            "groups": {
                "enabled": cfg_bool(cfg, |c| &c.groups, "enabled", true),
                "auto_join": cfg_bool(cfg, |c| &c.groups, "auto_join", true),
            },
            "push": {
                "enabled": cfg_bool(cfg, |c| &c.push, "enabled", true),
            },
            "management": {
                "enabled": cfg_bool(cfg, |c| &c.management, "enabled", true),
            },
            "blossom": {
                "enabled": cfg_bool(cfg, |c| &c.blossom, "enabled", blossom_default),
            },
            "livekit": {
                "enabled": cfg_bool(cfg, |c| &c.livekit, "enabled", livekit_default),
            },
            "roles": {
                "member": { "pubkeys": [], "can_invite": true, "can_manage": false }
            },
        });
        let auth = self.build_auth_header(&url, HttpMethod::POST).await?;

        let res = self
            .client
            .post(&url)
            .header(reqwest::header::AUTHORIZATION, auth)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow!("zooid create failed: {} {}", status, body));
        }

        Ok(())
    }

    /// Update a relay in zooid.
    ///
    /// PATCHes only the mutable fields (info + config sections).
    pub async fn update_relay(&self, relay: &Relay) -> Result<()> {
        let url = format!("{}/relay/{}", self.base_url.trim_end_matches('/'), relay.id);
        let host = format!("{}.{}", relay.subdomain, self.relay_domain);
        let blossom_default = relay.plan != "free";
        let livekit_default = relay.plan != "free";
        let cfg = relay.config.as_ref();
        let patch = json!({
            "host": host,
            "info": {
                "name": relay.name,
                "icon": relay.icon,
                "description": relay.description,
            },
            "policy": {
                "public_join": cfg_bool(cfg, |c| &c.policy, "public_join", false),
                "strip_signatures": cfg_bool(cfg, |c| &c.policy, "strip_signatures", false),
            },
            "groups": {
                "enabled": cfg_bool(cfg, |c| &c.groups, "enabled", true),
                "auto_join": cfg_bool(cfg, |c| &c.groups, "auto_join", true),
            },
            "push": {
                "enabled": cfg_bool(cfg, |c| &c.push, "enabled", true),
            },
            "management": {
                "enabled": cfg_bool(cfg, |c| &c.management, "enabled", true),
            },
            "blossom": {
                "enabled": cfg_bool(cfg, |c| &c.blossom, "enabled", blossom_default),
            },
            "livekit": {
                "enabled": cfg_bool(cfg, |c| &c.livekit, "enabled", livekit_default),
            },
        });
        let auth = self.build_auth_header(&url, HttpMethod::PATCH).await?;

        let res = self
            .client
            .patch(&url)
            .header(reqwest::header::AUTHORIZATION, auth)
            .json(&patch)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow!("zooid patch failed: {} {}", status, body));
        }

        Ok(())
    }

    async fn build_auth_header(&self, url: &str, method: HttpMethod) -> Result<String> {
        let url = Url::parse(url)?;
        let data = HttpData::new(url, method);
        let header = data.to_authorization(&self.admin_keys).await?;
        Ok(header)
    }
}

fn cfg_bool(
    cfg: Option<&RelayConfig>,
    section: impl Fn(&RelayConfig) -> &Option<Value>,
    key: &str,
    default: bool,
) -> bool {
    cfg.and_then(|c| section(c).as_ref())
        .and_then(|v| v[key].as_bool())
        .unwrap_or(default)
}


fn generate_secret_hex() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}
