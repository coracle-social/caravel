//! Custom domains: checking the ones tenants choose.

use std::sync::LazyLock;

use regex::Regex;

use crate::config::Config;
use crate::domain;
use crate::error::{Error, Result};

static CUSTOM_DOMAIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([a-z0-9][a-z0-9\-]*\.)+[a-z]{2,}$").unwrap());

/// Lowercase and check a custom domain, where empty means none. A domain under
/// one of the relay domains belongs in the subdomain instead, and the platform's
/// own hosts are no relay's.
pub fn normalize_custom_domain(config: &Config, domain: &str) -> Result<Option<String>> {
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

    if config.is_relay_domain(&domain) {
        return Err(Error::invalid(
            "reserved-domain",
            "use the subdomain field for domains under one of the platform's relay domains",
        ));
    }

    if is_platform_host(config, &domain) {
        return Err(Error::invalid(
            "reserved-domain",
            "the platform's own hosts can't serve a relay",
        ));
    }

    Ok(Some(domain))
}

/// Whether `host` is the one the API or the dashboards are served on.
pub fn is_platform_host(config: &Config, host: &str) -> bool {
    std::iter::once(&config.api_url)
        .chain(&config.platform_url)
        .any(|url| domain::relays::url_host(url) == host)
}
