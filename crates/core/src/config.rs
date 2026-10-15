//! The platform's configuration, as the app reads it from the environment.

use crate::crypto::PlatformKey;

#[derive(Clone, Debug)]
pub struct Config {
    /// The API's public base url, which NIP-98 `u` tags are checked against.
    pub api_url: String,
    pub admin_pubkeys: Vec<String>,
    /// Domains relay subdomains live under. A relay's canonical host is
    /// `<subdomain>.<domain>` for one of these.
    pub relay_domains: Vec<String>,
    pub platform_key: PlatformKey,
    pub blossom_s3: S3Config,
    pub livekit: LivekitConfig,
    /// The zooid API's host, which shares the relays' on-demand TLS.
    pub zooid_api_host: String,
}

#[derive(Clone, Debug, Default)]
pub struct S3Config {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Clone, Debug, Default)]
pub struct LivekitConfig {
    pub url: String,
    pub api_key: String,
    pub api_secret: String,
}

impl Config {
    /// Whether `pubkey` administers the platform.
    pub fn is_admin(&self, pubkey: &str) -> bool {
        self.admin_pubkeys.iter().any(|admin| admin == pubkey)
    }

    /// Whether `host` is one of the relay domains or sits under one.
    pub fn is_relay_domain(&self, host: &str) -> bool {
        self.relay_domains
            .iter()
            .any(|domain| host == domain || host.ends_with(&format!(".{domain}")))
    }
}
