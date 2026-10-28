//! The platform's configuration, as the app reads it from the environment.

use crate::nostr::Key;

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
    pub platform_key: Key,
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

use std::sync::LazyLock;

use regex::Regex;

use crate::error::{Error, Result};
use crate::urls::url_host;

static CUSTOM_DOMAIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([a-z0-9][a-z0-9\-]*\.)+[a-z]{2,}$").unwrap());

impl Config {
    /// Lowercase and check a custom domain, where empty means none. A domain under
    /// one of the relay domains belongs in the subdomain instead, and the platform's
    /// own hosts are no relay's.
    pub fn normalize_custom_domain(&self, domain: &str) -> Result<Option<String>> {
        let domain = domain.trim().trim_end_matches('.').to_lowercase();

        if domain.is_empty() {
            return Ok(None);
        }

        if !CUSTOM_DOMAIN_RE.is_match(&domain) {
            return Err(Error::invalid(
                "invalid-domain",
                "domain must be a valid hostname (e.g. relay.example.com)",
            ));
        }

        if self.is_relay_domain(&domain) {
            return Err(Error::invalid(
                "reserved-domain",
                "use the subdomain field for domains under one of the platform's relay domains",
            ));
        }

        if self.is_platform_host(&domain) {
            return Err(Error::invalid(
                "reserved-domain",
                "the platform's own hosts can't serve a relay",
            ));
        }

        Ok(Some(domain))
    }

    /// Whether `host` is the one the API or the dashboards are served on.
    pub fn is_platform_host(&self, host: &str) -> bool {
        std::iter::once(&self.api_url)
            .chain(&self.platform_url)
            .any(|url| url_host(url) == host)
    }
}
