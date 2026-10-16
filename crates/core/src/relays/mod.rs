//! Relays: the rows tenants create and the platform serves, with the hosts each
//! one answers on.

pub mod activity;

use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgExecutor};

use crate::error::{Error, Result};
use crate::{flag, plans};

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
    /// Events are served without signatures to readers who aren't members.
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
    #[serde(with = "crate::blank")]
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

const SELECT_RELAY: &str = "SELECT relay.*, ARRAY(
    SELECT host FROM host_redirect WHERE host_redirect.relay_id = relay.id ORDER BY host
) AS redirect_from FROM relay";

pub async fn list_relays(db: impl PgExecutor<'_>) -> Result<Vec<Relay>> {
    Ok(
        sqlx::query_as::<_, Relay>(&format!("{SELECT_RELAY} ORDER BY created_at DESC"))
            .fetch_all(db)
            .await?,
    )
}

pub async fn list_relays_for_tenant(
    db: impl PgExecutor<'_>,
    tenant_pubkey: &str,
) -> Result<Vec<Relay>> {
    Ok(sqlx::query_as::<_, Relay>(&format!(
        "{SELECT_RELAY} WHERE tenant_pubkey = $1 ORDER BY created_at DESC"
    ))
    .bind(tenant_pubkey)
    .fetch_all(db)
    .await?)
}

pub async fn get_relay(db: impl PgExecutor<'_>, id: &str) -> Result<Option<Relay>> {
    Ok(
        sqlx::query_as::<_, Relay>(&format!("{SELECT_RELAY} WHERE id = $1"))
            .bind(id)
            .fetch_optional(db)
            .await?,
    )
}

/// A relay that must exist.
pub async fn require_relay(db: impl PgExecutor<'_>, id: &str) -> Result<Relay> {
    get_relay(db, id)
        .await?
        .ok_or(Error::NotFound("relay not found"))
}

/// Lock a relay's row until the transaction ends, which serializes its joins.
pub async fn lock_relay(conn: &mut PgConnection, id: &str) -> Result<()> {
    sqlx::query("SELECT 1 FROM relay WHERE id = $1 FOR UPDATE")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
}

/// The live relay a host belongs to: as its canonical host, its verified custom
/// domain, or one of the hosts that redirect to it.
pub async fn get_relay_by_host(db: impl PgExecutor<'_>, host: &str) -> Result<Option<Relay>> {
    let (subdomain, domain) = host.split_once('.').unwrap_or((host, ""));

    Ok(sqlx::query_as::<_, Relay>(&format!(
        "{SELECT_RELAY} WHERE purged_at IS NULL AND (
            (subdomain = $1 AND domain = $2)
            OR (custom_domain = $3 AND custom_domain_verified)
            OR id = (SELECT relay_id FROM host_redirect WHERE host = $3)
        )"
    ))
    .bind(subdomain)
    .bind(domain)
    .bind(host)
    .fetch_optional(db)
    .await?)
}

pub async fn list_relays_with_custom_domain(
    db: impl PgExecutor<'_>,
    verified: bool,
) -> Result<Vec<Relay>> {
    Ok(sqlx::query_as::<_, Relay>(&format!(
        "{SELECT_RELAY} WHERE custom_domain IS NOT NULL AND custom_domain_verified = $1 AND purged_at IS NULL"
    ))
    .bind(verified)
    .fetch_all(db)
    .await?)
}

pub async fn insert_relay(conn: &mut PgConnection, relay: &Relay) -> Result<()> {
    sqlx::query(
        "INSERT INTO relay (
            id, tenant_pubkey, subdomain, domain, plan_id, status, pubkey, secret,
            info_name, info_icon, info_description,
            policy_public_read, policy_public_write, policy_public_join, policy_strip_signatures,
            groups_enabled, management_enabled, members_can_invite, blossom_enabled,
            livekit_enabled, push_enabled, custom_domain, created_at, policy_anonymous_read,
            blossom_authenticated_read, member_price, member_interval
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17,
            $18, $19, $20, $21, $22, $23, $24, $25, $26, $27)",
    )
    .bind(&relay.id)
    .bind(&relay.tenant_pubkey)
    .bind(&relay.subdomain)
    .bind(&relay.domain)
    .bind(&relay.plan_id)
    .bind(relay.status)
    .bind(&relay.pubkey)
    .bind(&relay.secret)
    .bind(&relay.info_name)
    .bind(&relay.info_icon)
    .bind(&relay.info_description)
    .bind(relay.policy_public_read)
    .bind(relay.policy_public_write)
    .bind(relay.policy_public_join)
    .bind(relay.policy_strip_signatures)
    .bind(relay.groups_enabled)
    .bind(relay.management_enabled)
    .bind(relay.members_can_invite)
    .bind(relay.blossom_enabled)
    .bind(relay.livekit_enabled)
    .bind(relay.push_enabled)
    .bind(&relay.custom_domain)
    .bind(relay.created_at)
    .bind(relay.policy_anonymous_read)
    .bind(relay.blossom_authenticated_read)
    .bind(relay.member_price)
    .bind(relay.member_interval)
    .execute(conn)
    .await?;
    Ok(())
}

/// Write a relay's settings. Its redirect hosts, status, keys and purge marker
/// have writers of their own. Verification of its custom domain is kept unless
/// the custom domain or the domain it must point under changed, comparing against
/// the stored row so a verification that landed since the relay was read survives.
pub async fn update_relay(conn: &mut PgConnection, relay: &Relay) -> Result<()> {
    sqlx::query(
        "UPDATE relay SET
            subdomain = $2, domain = $3, plan_id = $4,
            info_name = $5, info_icon = $6, info_description = $7,
            policy_public_read = $8, policy_public_write = $9,
            policy_public_join = $10, policy_strip_signatures = $11,
            groups_enabled = $12, management_enabled = $13, members_can_invite = $14,
            blossom_enabled = $15, livekit_enabled = $16, push_enabled = $17,
            custom_domain = $18,
            custom_domain_verified = COALESCE(
                custom_domain_verified AND custom_domain = $18 AND domain = $3 AND subdomain = $2,
                false
            ),
            policy_anonymous_read = $19,
            blossom_authenticated_read = $20, member_price = $21, member_interval = $22
         WHERE id = $1",
    )
    .bind(&relay.id)
    .bind(&relay.subdomain)
    .bind(&relay.domain)
    .bind(&relay.plan_id)
    .bind(&relay.info_name)
    .bind(&relay.info_icon)
    .bind(&relay.info_description)
    .bind(relay.policy_public_read)
    .bind(relay.policy_public_write)
    .bind(relay.policy_public_join)
    .bind(relay.policy_strip_signatures)
    .bind(relay.groups_enabled)
    .bind(relay.management_enabled)
    .bind(relay.members_can_invite)
    .bind(relay.blossom_enabled)
    .bind(relay.livekit_enabled)
    .bind(relay.push_enabled)
    .bind(&relay.custom_domain)
    .bind(relay.policy_anonymous_read)
    .bind(relay.blossom_authenticated_read)
    .bind(relay.member_price)
    .bind(relay.member_interval)
    .execute(conn)
    .await?;
    Ok(())
}

/// Change the name, description or icon the relay publishes, leaving `None`s as
/// they are.
pub async fn set_relay_info(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    name: Option<&str>,
    description: Option<&str>,
    icon: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE relay SET info_name = COALESCE($2, info_name), info_description = COALESCE($3, info_description),
            info_icon = COALESCE($4, info_icon)
         WHERE id = $1",
    )
    .bind(relay_id)
    .bind(name)
    .bind(description)
    .bind(icon)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_relay_status(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    status: RelayStatus,
) -> Result<()> {
    sqlx::query("UPDATE relay SET status = $1 WHERE id = $2")
        .bind(status)
        .bind(relay_id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn set_relay_purged_at(db: impl PgExecutor<'_>, relay_id: &str, at: i64) -> Result<()> {
    sqlx::query("UPDATE relay SET purged_at = $1 WHERE id = $2")
        .bind(at)
        .bind(relay_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Record whether DNS verification sees the relay's custom domain pointing at it,
/// but only if the custom domain is still `custom_domain`.
pub async fn set_custom_domain_verified(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    custom_domain: &str,
    verified: bool,
) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE relay SET custom_domain_verified = $1
         WHERE id = $2 AND custom_domain = $3 AND purged_at IS NULL",
    )
    .bind(verified)
    .bind(relay_id)
    .bind(custom_domain)
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Make `host` redirect to a relay, taking it from whichever relay it redirected to.
pub async fn add_host_redirect(
    db: impl PgExecutor<'_>,
    host: &str,
    relay_id: &str,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO host_redirect (host, relay_id, created_at) VALUES ($1, $2, $3)
         ON CONFLICT (host) DO UPDATE SET relay_id = EXCLUDED.relay_id, created_at = EXCLUDED.created_at",
    )
    .bind(host)
    .bind(relay_id)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

/// Stop redirecting the hosts a relay now answers on itself.
pub async fn remove_host_redirects(db: impl PgExecutor<'_>, hosts: &[String]) -> Result<()> {
    sqlx::query("DELETE FROM host_redirect WHERE host = ANY($1)")
        .bind(hosts)
        .execute(db)
        .await?;
    Ok(())
}

/// Record that a purged relay's data was erased.
pub async fn set_relay_erased_at(db: impl PgExecutor<'_>, id: &str, now: i64) -> Result<()> {
    sqlx::query("UPDATE relay SET erased_at = $2 WHERE id = $1")
        .bind(id)
        .bind(now)
        .execute(db)
        .await?;
    Ok(())
}

/// Purged relays whose data is still to be erased.
pub async fn list_unerased_relays(db: impl PgExecutor<'_>) -> Result<Vec<Relay>> {
    Ok(sqlx::query_as::<_, Relay>(&format!(
        "{SELECT_RELAY} WHERE purged_at IS NOT NULL AND erased_at IS NULL"
    ))
    .fetch_all(db)
    .await?)
}
