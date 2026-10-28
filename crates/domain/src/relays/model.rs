//! Relays: the rows tenants create and the platform serves, with the hosts each
//! one answers on, and the settings a tenant chooses for one.

use serde::{Deserialize, Serialize};

use crate::billing::plans;
use caravel_util::error::{Error, Result};
use caravel_util::flag::{self, Flag};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "text", rename_all = "lowercase")]
pub enum RelayStatus {
    /// Serving and billed.
    #[default]
    Active,
    /// Paused by its tenant, and not billed.
    Inactive,
    /// Paused because its tenant churned, until they pay or come back.
    Delinquent,
}

/// How often a membership renews.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "text", rename_all = "lowercase")]
pub enum MemberInterval {
    #[default]
    Month,
    Year,
}

impl MemberInterval {
    /// What Stripe calls it.
    pub fn as_str(self) -> &'static str {
        match self {
            MemberInterval::Month => "month",
            MemberInterval::Year => "year",
        }
    }

    /// Its length as NIP-11 states it: thirty days or 365.
    pub fn seconds(self) -> i64 {
        match self {
            MemberInterval::Month => 30 * 24 * 60 * 60,
            MemberInterval::Year => 365 * 24 * 60 * 60,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, sqlx::FromRow)]
pub struct Relay {
    pub id: String,
    pub tenant_pubkey: String,
    pub subdomain: String,
    /// Which of the relay domains this relay's canonical host sits under. Subdomains
    /// are unique across domains, so moving between them keeps the name reserved.
    #[serde(rename = "zooid_domain")]
    pub domain: String,
    pub plan_id: String,
    pub status: RelayStatus,
    /// The relay's own pubkey, which signs the events it publishes about itself.
    pub pubkey: String,
    /// The relay's secret key, encrypted with the platform key.
    #[serde(skip)]
    pub secret: String,
    pub info_name: String,
    pub info_icon: String,
    pub info_description: String,
    /// Anyone may read, without being a member.
    #[serde(with = "flag")]
    pub policy_public_read: bool,
    /// Anyone may read without authenticating. Implies nothing about who may
    /// read once authenticated, which `policy_public_read` decides.
    #[serde(with = "flag")]
    pub policy_anonymous_read: bool,
    /// Anyone may publish, without being a member.
    #[serde(with = "flag")]
    pub policy_public_write: bool,
    /// Anyone may join with a NIP-43 request, without an invite.
    #[serde(with = "flag")]
    pub policy_public_join: bool,
    /// Events are served without their signatures to everyone but the relay's
    /// owner and its own key.
    #[serde(with = "flag")]
    pub policy_strip_signatures: bool,
    #[serde(with = "flag")]
    pub groups_enabled: bool,
    #[serde(with = "flag")]
    pub management_enabled: bool,
    /// Members may list and create invite claims.
    #[serde(with = "flag")]
    pub members_can_invite: bool,
    #[serde(with = "flag")]
    pub blossom_enabled: bool,
    /// Only members may download the relay's media.
    #[serde(with = "flag")]
    pub blossom_authenticated_read: bool,
    #[serde(with = "flag")]
    pub livekit_enabled: bool,
    #[serde(with = "flag")]
    pub push_enabled: bool,
    /// A tenant's own domain for the relay, if they set one. It serves the relay
    /// once DNS verification sees it pointing at the canonical host.
    #[serde(with = "caravel_util::blank")]
    pub custom_domain: Option<String>,
    #[serde(with = "flag")]
    pub custom_domain_verified: bool,
    /// What a membership costs in cents per `member_interval`, or 0 if joining is
    /// free.
    pub member_price: i64,
    pub member_interval: MemberInterval,
    /// Hosts that redirect to the relay's current host, collected when a verified
    /// custom domain is replaced so its old links keep working.
    pub redirect_from: Vec<String>,
    /// When the relay was purged: its data deleted and its hosts released. The row
    /// stays as a record. Purging an active relay credits the tenant like a
    /// deactivation.
    pub purged_at: Option<i64>,
    /// When a purged relay's events, access lists, rooms and media were erased.
    pub erased_at: Option<i64>,
    pub created_at: i64,
}

/// What anyone may see of a relay: where it is and how it describes itself.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelayProfile {
    pub relay_id: String,
    pub host: String,
    /// Its name, or its host if it has none.
    pub name: String,
    pub icon: String,
    pub description: String,
}

impl Relay {
    /// The host the relay answers on: its verified custom domain if it has one,
    /// otherwise its canonical host.
    pub fn host(&self) -> String {
        match &self.custom_domain {
            Some(domain) if self.custom_domain_verified => domain.clone(),
            _ => self.canonical_host(),
        }
    }

    /// Every host the relay answers on: its canonical host, its verified custom
    /// domain, and the hosts that redirect to it.
    pub fn hosts(&self) -> Vec<String> {
        let mut hosts = vec![self.canonical_host()];
        if self.host() != hosts[0] {
            hosts.push(self.host());
        }
        hosts.extend(self.redirect_from.iter().cloned());
        hosts
    }

    /// `<subdomain>.<domain>`, which the relay always answers on and which a custom
    /// domain must point to before it is verified.
    pub fn canonical_host(&self) -> String {
        format!("{}.{}", self.subdomain, self.domain)
    }

    pub fn is_purged(&self) -> bool {
        self.purged_at.is_some()
    }

    /// Refuse to act on the relay once it is purged.
    pub fn require_unpurged(&self) -> Result<()> {
        if self.is_purged() {
            return Err(Error::conflict("relay-is-purged", "relay has been purged"));
        }
        Ok(())
    }

    /// Whether the platform serves the relay: it is active and not purged.
    pub fn is_served(&self) -> bool {
        self.status == RelayStatus::Active && !self.is_purged()
    }

    /// Whether the relay sells memberships: it has a price and is still served.
    pub fn charges_members(&self) -> bool {
        self.member_price > 0 && self.is_served()
    }

    /// The most members the relay's plan allows, if it has a limit.
    pub fn member_limit(&self) -> Option<i64> {
        plans::get_plan(&self.plan_id).and_then(|plan| plan.members)
    }

    /// What to call the relay in prose, falling back to its host.
    pub fn display_name(&self) -> String {
        if self.info_name.is_empty() {
            self.host()
        } else {
            self.info_name.clone()
        }
    }

    pub fn profile(&self) -> RelayProfile {
        RelayProfile {
            relay_id: self.id.clone(),
            host: self.host(),
            name: self.display_name(),
            icon: self.info_icon.clone(),
            description: self.info_description.clone(),
        }
    }
}

/// The settings a tenant chooses for a relay. Creating a relay fills the gaps with
/// defaults; updating one leaves them as they were.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RelaySettings {
    pub subdomain: Option<String>,
    #[serde(rename = "zooid_domain")]
    pub domain: Option<String>,
    pub plan_id: Option<String>,
    pub info_name: Option<String>,
    pub info_icon: Option<String>,
    pub info_description: Option<String>,
    pub policy_public_read: Option<Flag>,
    pub policy_anonymous_read: Option<Flag>,
    pub policy_public_write: Option<Flag>,
    pub policy_public_join: Option<Flag>,
    pub policy_strip_signatures: Option<Flag>,
    pub groups_enabled: Option<Flag>,
    pub management_enabled: Option<Flag>,
    pub members_can_invite: Option<Flag>,
    pub blossom_enabled: Option<Flag>,
    pub blossom_authenticated_read: Option<Flag>,
    pub livekit_enabled: Option<Flag>,
    pub push_enabled: Option<Flag>,
    pub custom_domain: Option<String>,
    pub member_price: Option<i64>,
    pub member_interval: Option<MemberInterval>,
}
