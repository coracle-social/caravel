//! Reading and checking the environment, the only place configuration comes from.

use anyhow::{Context, Result, bail};
use caravel_adapters::blobs::S3Config;
use caravel_adapters::robot::RobotProfile;
use caravel_core::config::Config;
use caravel_core::crypto::PlatformKey;
use caravel_relay::RelaysConfig;

/// Everything the app is configured with: the core's config, plus what the app
/// needs to connect the database and build the adapters.
pub struct Settings {
    pub port: u16,
    pub database_url: String,
    pub config: Config,
    pub robot_secret: String,
    pub robot_profile: RobotProfile,
    pub robot_wallet: String,
    pub stripe_secret_key: String,
    pub media: Media,
    pub livekit: Option<Livekit>,
    pub relays: RelaysConfig,
}

/// Where relays' media is kept.
pub enum Media {
    Local(String),
    S3(S3Config),
}

/// The LiveKit server calls run on.
pub struct Livekit {
    pub url: String,
    pub api_key: String,
    pub api_secret: String,
}

pub fn load() -> Result<Settings> {
    let robot_secret = required("ROBOT_SECRET")?;
    let platform_key =
        PlatformKey::from_hex(&robot_secret).context("ROBOT_SECRET is not a nostr secret key")?;

    let api_url = required("API_URL")?.trim_end_matches('/').to_string();
    url::Url::parse(&api_url).context("API_URL is not a url")?;

    Ok(Settings {
        port: optional("PORT")
            .map_or(Ok(2892), |port| port.parse().context("PORT is not a port"))?,
        database_url: required("DATABASE_URL")?,
        config: Config {
            api_url,
            admin_pubkeys: list("ADMIN_PUBKEYS"),
            relay_domains: required_list("RELAY_DOMAINS")?,
            platform_key,
        },
        robot_secret,
        robot_profile: RobotProfile {
            name: optional("ROBOT_NAME").unwrap_or_default(),
            picture: optional("ROBOT_PICTURE").unwrap_or_default(),
            description: optional("ROBOT_DESCRIPTION").unwrap_or_default(),
            outbox_relays: required_list("ROBOT_OUTBOX_RELAYS")?,
            indexer_relays: required_list("ROBOT_INDEXER_RELAYS")?,
            messaging_relays: required_list("ROBOT_MESSAGING_RELAYS")?,
        },
        robot_wallet: required("ROBOT_WALLET")?,
        stripe_secret_key: required("STRIPE_SECRET_KEY")?,
        media: match optional("MEDIA_S3_BUCKET") {
            Some(bucket) => Media::S3(S3Config {
                endpoint: optional("MEDIA_S3_ENDPOINT").unwrap_or_default(),
                region: required("MEDIA_S3_REGION")?,
                bucket,
                access_key: required("MEDIA_S3_ACCESS_KEY")?,
                secret_key: required("MEDIA_S3_SECRET_KEY")?,
            }),
            None => Media::Local(optional("MEDIA_DIR").unwrap_or_else(|| "data/media".to_string())),
        },
        livekit: match optional("LIVEKIT_URL") {
            Some(url) => Some(Livekit {
                url,
                api_key: required("LIVEKIT_API_KEY")?,
                api_secret: required("LIVEKIT_API_SECRET")?,
            }),
            None => None,
        },
        relays: RelaysConfig {
            forwarded_header: optional("FORWARDED_HEADER"),
            connections_per_minute: optional("CONNECTIONS_PER_MINUTE").map_or(Ok(0), |value| {
                value
                    .parse()
                    .context("CONNECTIONS_PER_MINUTE is not a number")
            })?,
            ..Default::default()
        },
    })
}

#[allow(clippy::disallowed_methods)]
fn optional(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn required(key: &str) -> Result<String> {
    optional(key).with_context(|| format!("{key} is required"))
}

fn list(key: &str) -> Vec<String> {
    optional(key)
        .unwrap_or_default()
        .split(',')
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn required_list(key: &str) -> Result<Vec<String>> {
    let values = list(key);
    if values.is_empty() {
        bail!("{key} is required");
    }
    Ok(values)
}
