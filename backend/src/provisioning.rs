use anyhow::{anyhow, Result};
use rand::rngs::OsRng;
use rand::RngCore;
use reqwest::Client;
use serde::Serialize;

use nostr_sdk::nostr::nips::nip98::{HttpData, HttpMethod};
use nostr_sdk::nostr::types::url::Url;
use nostr_sdk::nostr::Keys;

use crate::models::NewRelay;

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

        let admin_keys = Keys::parse(admin_secret)?;
        let client = Client::new();

        Ok(Self {
            base_url,
            relay_domain,
            admin_keys,
            client,
        })
    }

    pub async fn provision_relay(&self, relay: &NewRelay) -> Result<()> {
        let host = format!("{}.{}", relay.subdomain, self.relay_domain);
        let secret = generate_secret_hex();

        let payload = ZooidConfig::new(relay, host, secret);
        let url = format!("{}/relay/{}", self.base_url.trim_end_matches('/'), relay.id);
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
            return Err(anyhow!("zooid provisioning failed: {} {}", status, body));
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

#[derive(Debug, Serialize)]
struct ZooidConfig {
    host: String,
    schema: String,
    secret: String,
    info: ZooidInfo,
    policy: ZooidPolicy,
    groups: ZooidGroups,
    push: ZooidPush,
    management: ZooidManagement,
    blossom: ZooidBlossom,
    roles: ZooidRoles,
}

impl ZooidConfig {
    fn new(relay: &NewRelay, host: String, secret: String) -> Self {
        let blossom_enabled = relay.plan != "free";

        Self {
            host,
            schema: relay.schema.clone(),
            secret,
            info: ZooidInfo {
                name: relay.name.clone(),
                icon: relay.icon.clone(),
                pubkey: relay.tenant.clone(),
                description: relay.description.clone(),
            },
            policy: ZooidPolicy {
                public_join: false,
                strip_signatures: false,
            },
            groups: ZooidGroups {
                enabled: true,
                auto_join: true,
            },
            push: ZooidPush { enabled: true },
            management: ZooidManagement {
                enabled: true,
                methods: Vec::new(),
            },
            blossom: ZooidBlossom {
                enabled: blossom_enabled,
            },
            roles: ZooidRoles {
                member: ZooidRole {
                    pubkeys: Vec::new(),
                    can_invite: true,
                    can_manage: false,
                },
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct ZooidInfo {
    name: String,
    icon: String,
    pubkey: String,
    description: String,
}

#[derive(Debug, Serialize)]
struct ZooidPolicy {
    public_join: bool,
    strip_signatures: bool,
}

#[derive(Debug, Serialize)]
struct ZooidGroups {
    enabled: bool,
    auto_join: bool,
}

#[derive(Debug, Serialize)]
struct ZooidPush {
    enabled: bool,
}

#[derive(Debug, Serialize)]
struct ZooidManagement {
    enabled: bool,
    methods: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ZooidBlossom {
    enabled: bool,
}

#[derive(Debug, Serialize)]
struct ZooidRoles {
    member: ZooidRole,
}

#[derive(Debug, Serialize)]
struct ZooidRole {
    pubkeys: Vec<String>,
    can_invite: bool,
    can_manage: bool,
}

fn generate_secret_hex() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}
