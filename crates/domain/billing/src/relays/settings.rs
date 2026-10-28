//! The rules for a relay's settings and the hosts it answers on.

use std::sync::LazyLock;

use regex::Regex;

use super::plans;
use super::{Relay, RelaySettings};
use caravel_util::config::Config;
use caravel_util::error::{Error, Result};
use caravel_util::flag::Flag;

const RESERVED_SUBDOMAINS: [&str; 3] = ["api", "admin", "internal"];

/// The least Stripe charges, in cents.
const MIN_MEMBER_PRICE: i64 = 50;

static SUBDOMAIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]{1,61}[a-z0-9]$").unwrap());

impl Relay {
    /// Overlay chosen settings on a relay. Changing the subdomain, the domain or
    /// the custom domain withdraws the custom domain's verification.
    pub fn apply_settings(mut self, config: &Config, settings: RelaySettings) -> Result<Relay> {
        let flag = |target: &mut bool, value: Option<Flag>| {
            if let Some(Flag(value)) = value {
                *target = value;
            }
        };

        if let Some(subdomain) = settings.subdomain
            && subdomain != self.subdomain
        {
            self.subdomain = subdomain;
            self.custom_domain_verified = false;
        }
        if let Some(domain) = settings.domain {
            let domain = domain.trim().to_lowercase();
            if domain != self.domain {
                self.domain = domain;
                self.custom_domain_verified = false;
            }
        }
        if let Some(plan_id) = settings.plan_id {
            self.plan_id = plan_id;
        }
        if let Some(name) = settings.info_name {
            self.info_name = name;
        }
        if let Some(icon) = settings.info_icon {
            self.info_icon = icon;
        }
        if let Some(description) = settings.info_description {
            self.info_description = description;
        }
        flag(&mut self.policy_public_read, settings.policy_public_read);
        flag(
            &mut self.policy_anonymous_read,
            settings.policy_anonymous_read,
        );
        flag(&mut self.policy_public_write, settings.policy_public_write);
        flag(&mut self.policy_public_join, settings.policy_public_join);
        flag(
            &mut self.policy_strip_signatures,
            settings.policy_strip_signatures,
        );
        flag(&mut self.groups_enabled, settings.groups_enabled);
        flag(&mut self.management_enabled, settings.management_enabled);
        flag(&mut self.members_can_invite, settings.members_can_invite);
        flag(&mut self.blossom_enabled, settings.blossom_enabled);
        flag(
            &mut self.blossom_authenticated_read,
            settings.blossom_authenticated_read,
        );
        flag(&mut self.livekit_enabled, settings.livekit_enabled);
        flag(&mut self.push_enabled, settings.push_enabled);
        if let Some(price) = settings.member_price {
            self.member_price = price;
        }
        if let Some(interval) = settings.member_interval {
            self.member_interval = interval;
        }
        if let Some(custom_domain) = settings.custom_domain {
            let custom_domain = config.normalize_custom_domain(&custom_domain)?;
            if custom_domain != self.custom_domain {
                self.custom_domain = custom_domain;
                self.custom_domain_verified = false;
            }
        }

        Ok(self)
    }

    /// Check a relay's subdomain, domain and plan, that the plan allows the premium
    /// features it has on, and that its member price is one Stripe can charge for a
    /// relay that doesn't let anyone in for free.
    pub fn validate(self, config: &Config) -> Result<Relay> {
        if !SUBDOMAIN_RE.is_match(&self.subdomain)
            || RESERVED_SUBDOMAINS.contains(&self.subdomain.as_str())
            || config.is_platform_host(&self.canonical_host())
        {
            return Err(Error::invalid("invalid-subdomain", "subdomain is invalid"));
        }

        if !config.relay_domains.contains(&self.domain) {
            return Err(Error::invalid(
                "invalid-zooid-domain",
                "zooid_domain is not a domain this platform serves",
            ));
        }

        let plan = plans::get_plan(&self.plan_id)
            .ok_or_else(|| Error::invalid("invalid-plan", "plan not found"))?;
        if (!plan.blossom && self.blossom_enabled) || (!plan.livekit && self.livekit_enabled) {
            return Err(Error::invalid(
                "premium-feature",
                "feature requires a paid plan",
            ));
        }

        if self.member_price != 0 && self.member_price < MIN_MEMBER_PRICE {
            return Err(Error::invalid(
                "invalid-member-price",
                "a member price must be free or at least $0.50",
            ));
        }

        if self.member_price > 0 && self.policy_public_join {
            return Err(Error::invalid(
                "paid-public-join",
                "a relay anyone may join can't charge for membership",
            ));
        }

        if self.policy_anonymous_read && !self.policy_public_read {
            return Err(Error::invalid(
                "anonymous-read-needs-public-read",
                "a relay that reads without authenticating has to be readable by anyone",
            ));
        }

        Ok(self)
    }
}
