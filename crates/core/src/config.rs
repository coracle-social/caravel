//! The platform's configuration, as the app reads it from the environment.

use crate::crypto::PlatformKey;

#[derive(Clone, Debug)]
pub struct Config {
    /// The API's public base url, which NIP-98 `u` tags are checked against.
    pub api_url: String,
    /// The dashboards' public url. Without one, this process serves no dashboard.
    pub platform_url: Option<String>,
    pub admin_pubkeys: Vec<String>,
    /// Domains relay subdomains live under. A relay's canonical host is
    /// `<subdomain>.<domain>` for one of these.
    pub relay_domains: Vec<String>,
    pub platform_key: PlatformKey,
    /// The platform's cut of each membership payment, as a percent with at most two
    /// decimal places.
    pub member_fee_percent: f64,
    /// The Flotilla instance invites are accepted in.
    pub flotilla_url: String,
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
