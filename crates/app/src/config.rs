//! Reading and checking the environment, the only place configuration comes from.

use anyhow::{Context, Result, bail};
use caravel_util::config::Config;
use caravel_relay::RelaysConfig;
use caravel_util::blobs::S3Config;
use caravel_util::nostr::Key;
use caravel_util::robot::RobotProfile;

/// Everything the app is configured with: the core's config, plus what the app
/// needs to connect the database and build the adapters.
pub struct Settings {
    pub port: u16,
    pub database_url: String,
    pub config: Config,
    pub robot_profile: RobotProfile,
    pub robot_wallet: String,
    pub stripe_secret_key: String,
    pub media: Media,
    pub livekit: Option<Livekit>,
    pub relays: RelaysConfig,
    pub web: WebSettings,
}

/// How the dashboards and the member portal are branded.
pub struct WebSettings {
    pub platform_name: String,
    pub platform_logo: String,
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
        Key::from_hex(&robot_secret).context("ROBOT_SECRET is not a nostr secret key")?;

    let api_url = required("API_URL")?.trim_end_matches('/').to_string();
    url::Url::parse(&api_url).context("API_URL is not a url")?;

    Ok(Settings {
        port: optional("PORT")
            .map_or(Ok(2892), |port| port.parse().context("PORT is not a port"))?,
        database_url: required("DATABASE_URL")?,
        config: Config {
            api_url,
            platform_url: optional("PLATFORM_URL").map(|url| url.trim_end_matches('/').to_string()),
            admin_pubkeys: list("ADMIN_PUBKEYS"),
            relay_domains: required_list("RELAY_DOMAINS")?,
            platform_key,
            member_fee_percent: optional("MEMBER_FEE_PERCENT").map_or(Ok(10.0), |value| {
                parse_percent(&value).context(
                    "MEMBER_FEE_PERCENT is not a percent from 0 to 100 with at most two decimal places",
                )
            })?,
            flotilla_url: optional("FLOTILLA_URL")
                .unwrap_or_else(|| "https://app.flotilla.social".to_string())
                .trim_end_matches('/')
                .to_string(),
        },
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
        web: WebSettings {
            platform_name: optional("PLATFORM_NAME").unwrap_or_else(|| "Caravel".to_string()),
            platform_logo: optional("PLATFORM_LOGO").unwrap_or_else(|| "/caravel.png".to_string()),
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

/// A percent from 0 to 100 with at most two decimal places, which is what Stripe
/// takes as an application fee.
fn parse_percent(raw: &str) -> Option<f64> {
    let decimals = raw
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let percent: f64 = raw.parse().ok()?;
    (decimals <= 2 && (0.0..=100.0).contains(&percent)).then_some(percent)
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

#[cfg(test)]
mod tests {
    use super::parse_percent;

    #[test]
    fn a_fee_percent_is_what_stripe_accepts() {
        assert_eq!(parse_percent("10"), Some(10.0));
        assert_eq!(parse_percent("2.55"), Some(2.55));
        assert_eq!(parse_percent("0"), Some(0.0));
        assert_eq!(parse_percent("100"), Some(100.0));
        assert_eq!(parse_percent("2.555"), None);
        assert_eq!(parse_percent("100.01"), None);
        assert_eq!(parse_percent("-1"), None);
        assert_eq!(parse_percent("NaN"), None);
        assert_eq!(parse_percent("ten"), None);
    }
}
