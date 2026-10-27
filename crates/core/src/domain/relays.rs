//! The rules for a relay's settings and the hosts it answers on.

use std::sync::LazyLock;

use regex::Regex;

use crate::config::Config;
use crate::domain;
use crate::error::{Error, Result};
use crate::flag::Flag;
use crate::models::plans;
use crate::models::relays::{Relay, RelaySettings};

/// The host a url or `Host` header names, lowercased, without scheme, port, path
/// or trailing dot.
pub fn url_host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or_default();
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, port)| {
            if port.chars().all(|c| c.is_ascii_digit()) {
                host
            } else {
                authority
            }
        });
    host.trim_end_matches('.').to_lowercase()
}

const RESERVED_SUBDOMAINS: [&str; 3] = ["api", "admin", "internal"];

/// The least Stripe charges, in cents.
const MIN_MEMBER_PRICE: i64 = 50;

static SUBDOMAIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]{1,61}[a-z0-9]$").unwrap());

/// Overlay chosen settings on a relay. Changing the subdomain, the domain or the
/// custom domain withdraws the custom domain's verification.
pub fn apply_settings(config: &Config, mut relay: Relay, settings: RelaySettings) -> Result<Relay> {
    let flag = |target: &mut bool, value: Option<Flag>| {
        if let Some(Flag(value)) = value {
            *target = value;
        }
    };

    if let Some(subdomain) = settings.subdomain
        && subdomain != relay.subdomain
    {
        relay.subdomain = subdomain;
        relay.custom_domain_verified = false;
    }
    if let Some(domain) = settings.domain {
        let domain = domain.trim().to_lowercase();
        if domain != relay.domain {
            relay.domain = domain;
            relay.custom_domain_verified = false;
        }
    }
    if let Some(plan_id) = settings.plan_id {
        relay.plan_id = plan_id;
    }
    if let Some(name) = settings.info_name {
        relay.info_name = name;
    }
    if let Some(icon) = settings.info_icon {
        relay.info_icon = icon;
    }
    if let Some(description) = settings.info_description {
        relay.info_description = description;
    }
    flag(&mut relay.policy_public_read, settings.policy_public_read);
    flag(
        &mut relay.policy_anonymous_read,
        settings.policy_anonymous_read,
    );
    flag(&mut relay.policy_public_write, settings.policy_public_write);
    flag(&mut relay.policy_public_join, settings.policy_public_join);
    flag(
        &mut relay.policy_strip_signatures,
        settings.policy_strip_signatures,
    );
    flag(&mut relay.groups_enabled, settings.groups_enabled);
    flag(&mut relay.management_enabled, settings.management_enabled);
    flag(&mut relay.members_can_invite, settings.members_can_invite);
    flag(&mut relay.blossom_enabled, settings.blossom_enabled);
    flag(
        &mut relay.blossom_authenticated_read,
        settings.blossom_authenticated_read,
    );
    flag(&mut relay.livekit_enabled, settings.livekit_enabled);
    flag(&mut relay.push_enabled, settings.push_enabled);
    if let Some(price) = settings.member_price {
        relay.member_price = price;
    }
    if let Some(interval) = settings.member_interval {
        relay.member_interval = interval;
    }
    if let Some(custom_domain) = settings.custom_domain {
        let custom_domain = domain::domains::normalize_custom_domain(config, &custom_domain)?;
        if custom_domain != relay.custom_domain {
            relay.custom_domain = custom_domain;
            relay.custom_domain_verified = false;
        }
    }

    Ok(relay)
}

/// Check a relay's subdomain, domain and plan, that the plan allows the premium
/// features it has on, and that its member price is one Stripe can charge for a
/// relay that doesn't let anyone in for free.
pub fn validate_relay(config: &Config, relay: Relay) -> Result<Relay> {
    if !SUBDOMAIN_RE.is_match(&relay.subdomain)
        || RESERVED_SUBDOMAINS.contains(&relay.subdomain.as_str())
        || domain::domains::is_platform_host(config, &relay.canonical_host())
    {
        return Err(Error::invalid("invalid-subdomain", "subdomain is invalid"));
    }

    if !config.relay_domains.contains(&relay.domain) {
        return Err(Error::invalid(
            "invalid-zooid-domain",
            "zooid_domain is not a domain this platform serves",
        ));
    }

    let plan = plans::get_plan(&relay.plan_id)
        .ok_or_else(|| Error::invalid("invalid-plan", "plan not found"))?;
    if (!plan.blossom && relay.blossom_enabled) || (!plan.livekit && relay.livekit_enabled) {
        return Err(Error::invalid(
            "premium-feature",
            "feature requires a paid plan",
        ));
    }

    if relay.member_price != 0 && relay.member_price < MIN_MEMBER_PRICE {
        return Err(Error::invalid(
            "invalid-member-price",
            "a member price must be free or at least $0.50",
        ));
    }

    if relay.member_price > 0 && relay.policy_public_join {
        return Err(Error::invalid(
            "paid-public-join",
            "a relay anyone may join can't charge for membership",
        ));
    }

    Ok(relay)
}
