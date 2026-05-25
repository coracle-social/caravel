use anyhow::{Result, anyhow};
use nostr_sdk::prelude::*;

#[derive(Clone)]
pub struct Env {
    pub server_host: String,
    pub server_port: u16,
    pub server_admin_pubkeys: Vec<String>,
    pub server_allow_origins: Vec<String>,
    pub app_url: String,
    pub database_url: String,
    pub robot_name: String,
    pub robot_wallet: String,
    pub robot_picture: String,
    pub robot_description: String,
    pub robot_outbox_relays: Vec<String>,
    pub robot_indexer_relays: Vec<String>,
    pub robot_messaging_relays: Vec<String>,
    pub blossom_s3_region: String,
    pub blossom_s3_bucket: String,
    pub blossom_s3_endpoint: String,
    pub blossom_s3_access_key: String,
    pub blossom_s3_secret_key: String,
    pub zooid_api_url: String,
    pub relay_domain: String,
    pub livekit_url: String,
    pub livekit_api_key: String,
    pub livekit_api_secret: String,
    pub stripe_secret_key: String,
    pub stripe_webhook_secret: String,
    pub stripe_price_basic: String,
    pub stripe_price_growth: String,
    /// Parsed from `robot_secret`; used for nostr signing and nip44 encryption.
    pub keys: Keys,
}

impl Env {
    pub fn load() -> Self {
        let keys = Keys::parse(&require_str("ROBOT_SECRET"))
            .expect("ROBOT_SECRET is not a valid nostr secret key");

        Self {
            server_host: require_str("SERVER_HOST"),
            server_port: require_u16("SERVER_PORT"),
            server_admin_pubkeys: require_csv("SERVER_ADMIN_PUBKEYS"),
            server_allow_origins: require_csv("SERVER_ALLOW_ORIGINS"),
            app_url: require_str("APP_URL").trim_end_matches('/').to_string(),
            database_url: require_str("DATABASE_URL"),
            robot_name: require_str("ROBOT_NAME"),
            robot_wallet: require_str("ROBOT_WALLET"),
            robot_picture: require_str("ROBOT_PICTURE"),
            robot_description: require_str("ROBOT_DESCRIPTION"),
            robot_outbox_relays: require_csv("ROBOT_OUTBOX_RELAYS"),
            robot_indexer_relays: require_csv("ROBOT_INDEXER_RELAYS"),
            robot_messaging_relays: require_csv("ROBOT_MESSAGING_RELAYS"),
            blossom_s3_region: require_str("BLOSSOM_S3_REGION"),
            blossom_s3_bucket: require_str("BLOSSOM_S3_BUCKET"),
            blossom_s3_endpoint: require_str("BLOSSOM_S3_ENDPOINT"),
            blossom_s3_access_key: require_str("BLOSSOM_S3_ACCESS_KEY"),
            blossom_s3_secret_key: require_str("BLOSSOM_S3_SECRET_KEY"),
            zooid_api_url: require_str("ZOOID_API_URL"),
            relay_domain: require_str("RELAY_DOMAIN"),
            livekit_url: require_str("LIVEKIT_URL"),
            livekit_api_key: require_str("LIVEKIT_API_KEY"),
            livekit_api_secret: require_str("LIVEKIT_API_SECRET"),
            stripe_secret_key: require_str("STRIPE_SECRET_KEY"),
            stripe_webhook_secret: require_str("STRIPE_WEBHOOK_SECRET"),
            stripe_price_basic: require_str("STRIPE_PRICE_BASIC"),
            stripe_price_growth: require_str("STRIPE_PRICE_GROWTH"),
            keys,
        }
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        nip44::encrypt(
            self.keys.secret_key(),
            &self.keys.public_key(),
            plaintext,
            nip44::Version::V2,
        )
        .map_err(|e| anyhow!("encryption failed: {e}"))
    }

    pub fn decrypt(&self, ciphertext: &str) -> Result<String> {
        nip44::decrypt(self.keys.secret_key(), &self.keys.public_key(), ciphertext)
            .map_err(|e| anyhow!("decryption failed: {e}"))
    }

    pub async fn make_auth(&self, url: &str, method: HttpMethod) -> Result<String> {
        let server_url = Url::parse(url)?;
        let auth = HttpData::new(server_url, method)
            .to_authorization(&self.keys)
            .await?;
        Ok(auth)
    }
}

fn require_str(key: &str) -> String {
    let v = std::env::var(key)
        .unwrap_or_else(|_| panic!("{key} is required"))
        .trim()
        .to_string();
    if v.is_empty() {
        panic!("{key} is required")
    }
    v
}

fn require_u16(key: &str) -> u16 {
    require_str(key)
        .parse()
        .unwrap_or_else(|_| panic!("{key} is invalid"))
}

fn require_csv(key: &str) -> Vec<String> {
    let v: Vec<String> = std::env::var(key)
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if v.is_empty() {
        panic!("{key} is required");
    }

    v
}
