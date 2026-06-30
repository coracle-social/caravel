use std::sync::{Arc, LazyLock};

use anyhow::Result;
use axum::{
    Json,
    extract::{Path, State},
};
use regex::Regex;
use serde::Deserialize;

use crate::api::{Api, AuthedPubkey};
use crate::domains;
use crate::env;
use crate::models::{RELAY_STATUS_ACTIVE, RELAY_STATUS_DELINQUENT, RELAY_STATUS_INACTIVE, Relay};
use crate::web::{
    ApiError, ApiResult, bad_request, created, internal, map_unique_error, ok, parse_bool_default,
    unprocessable,
};
use crate::{command, infra, query};

pub async fn list_relays(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
) -> ApiResult {
    api.require_admin(&auth)?;

    let relays = query::list_relays().await.map_err(internal)?;
    ok(relays)
}

pub async fn get_relay(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = api.get_relay_or_404(&id).await?;
    api.require_admin_or_tenant(&auth, &relay.tenant_pubkey)?;
    ok(relay)
}

pub async fn list_relay_activity(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = api.get_relay_or_404(&id).await?;
    api.require_admin_or_tenant(&auth, &relay.tenant_pubkey)?;

    let activity = query::list_activity_for_resource(&id)
        .await
        .map_err(internal)?;
    ok(serde_json::json!({ "activity": activity }))
}

pub async fn list_relay_members(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = api.get_relay_or_404(&id).await?;
    api.require_admin_or_tenant(&auth, &relay.tenant_pubkey)?;

    let members = fetch_relay_members(&relay).await.map_err(internal)?;
    ok(serde_json::json!({ "members": members }))
}

#[derive(Deserialize)]
pub struct CreateRelayRequest {
    pub tenant_pubkey: String,
    pub subdomain: String,
    pub plan_id: String,
    pub info_name: String,
    pub info_icon: String,
    pub info_description: String,
    pub policy_public_read: i64,
    pub policy_public_write: i64,
    pub policy_public_join: i64,
    pub policy_strip_signatures: i64,
    pub groups_enabled: i64,
    pub management_enabled: i64,
    pub blossom_enabled: i64,
    pub livekit_enabled: i64,
    pub push_enabled: i64,
    #[serde(default)]
    pub custom_domain: String,
}

pub async fn create_relay(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Json(payload): Json<CreateRelayRequest>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &payload.tenant_pubkey)?;

    let relay_id = format!(
        "{}_{}",
        payload.subdomain.replace('-', "_"),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );

    let relay = Relay {
        id: relay_id.clone(),
        tenant_pubkey: payload.tenant_pubkey,
        subdomain: payload.subdomain,
        plan_id: payload.plan_id,
        info_name: payload.info_name,
        info_icon: payload.info_icon,
        info_description: payload.info_description,
        policy_public_read: payload.policy_public_read,
        policy_public_write: payload.policy_public_write,
        policy_public_join: payload.policy_public_join,
        policy_strip_signatures: payload.policy_strip_signatures,
        groups_enabled: payload.groups_enabled,
        management_enabled: payload.management_enabled,
        blossom_enabled: payload.blossom_enabled,
        livekit_enabled: payload.livekit_enabled,
        push_enabled: payload.push_enabled,
        custom_domain: normalize_custom_domain(&payload.custom_domain)?,
        ..Default::default()
    };

    let relay = prepare_relay(relay)?;

    command::create_relay(&relay)
        .await
        .map_err(map_relay_write_error)?;

    created(relay)
}

#[derive(Deserialize)]
pub struct UpdateRelayRequest {
    pub subdomain: Option<String>,
    pub plan_id: Option<String>,
    pub info_name: Option<String>,
    pub info_icon: Option<String>,
    pub info_description: Option<String>,
    pub policy_public_read: Option<i64>,
    pub policy_public_write: Option<i64>,
    pub policy_public_join: Option<i64>,
    pub policy_strip_signatures: Option<i64>,
    pub groups_enabled: Option<i64>,
    pub management_enabled: Option<i64>,
    pub blossom_enabled: Option<i64>,
    pub livekit_enabled: Option<i64>,
    pub push_enabled: Option<i64>,
    pub custom_domain: Option<String>,
}

pub async fn update_relay(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
    Json(payload): Json<UpdateRelayRequest>,
) -> ApiResult {
    let mut relay = api.get_relay_or_404(&id).await?;

    api.require_admin_or_tenant(&auth, &relay.tenant_pubkey)?;

    let current_plan = relay.plan_id.clone();
    let requested_plan = payload.plan_id.clone();

    if let Some(v) = payload.subdomain {
        relay.subdomain = v;
    }
    if let Some(v) = requested_plan.clone() {
        relay.plan_id = v;
    }
    if let Some(v) = payload.info_name {
        relay.info_name = v;
    }
    if let Some(v) = payload.info_icon {
        relay.info_icon = v;
    }
    if let Some(v) = payload.info_description {
        relay.info_description = v;
    }
    if let Some(v) = payload.policy_public_read {
        relay.policy_public_read = v;
    }
    if let Some(v) = payload.policy_public_write {
        relay.policy_public_write = v;
    }
    if let Some(v) = payload.policy_public_join {
        relay.policy_public_join = v;
    }
    if let Some(v) = payload.policy_strip_signatures {
        relay.policy_strip_signatures = v;
    }
    if let Some(v) = payload.groups_enabled {
        relay.groups_enabled = v;
    }
    if let Some(v) = payload.management_enabled {
        relay.management_enabled = v;
    }
    if let Some(v) = payload.blossom_enabled {
        relay.blossom_enabled = v;
    }
    if let Some(v) = payload.livekit_enabled {
        relay.livekit_enabled = v;
    }
    if let Some(v) = payload.push_enabled {
        relay.push_enabled = v;
    }
    // Changing the custom domain invalidates any prior verification, so clear the
    // flag and let the background poller re-verify the new domain. An update that
    // round-trips the existing domain (e.g. a feature toggle) leaves it untouched;
    // the matching reset in `command::update_relay` keeps the persisted row right.
    if let Some(v) = payload.custom_domain {
        let normalized = normalize_custom_domain(&v)?;
        if normalized != relay.custom_domain {
            relay.custom_domain = normalized;
            relay.custom_domain_verified = 0;
        }
    }

    let relay = prepare_relay(relay)?;

    let plan_changed = requested_plan
        .as_deref()
        .is_some_and(|requested| requested != current_plan);

    if plan_changed {
        let selected_plan = query::get_plan(&relay.plan_id).map_err(internal)?;
        if let Some(limit) = selected_plan.members {
            let current_members = fetch_relay_members(&relay).await.map_err(internal)?.len() as i64;

            if current_members > limit {
                let message = format!(
                    "relay has {current_members} members, which exceeds the {} plan limit of {limit}",
                    selected_plan.name.to_lowercase()
                );
                return Err(unprocessable("member-limit-exceeded", &message));
            }
        }
    }

    command::update_relay(&relay)
        .await
        .map_err(map_relay_write_error)?;

    ok(relay)
}

pub async fn deactivate_relay(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = api.get_relay_or_404(&id).await?;
    api.require_admin_or_tenant(&auth, &relay.tenant_pubkey)?;

    if relay.status == RELAY_STATUS_DELINQUENT {
        return Err(bad_request("relay-is-delinquent", "relay is delinquent"));
    }

    if relay.status == RELAY_STATUS_INACTIVE {
        return Err(bad_request(
            "relay-is-inactive",
            "relay is already inactive",
        ));
    }

    command::deactivate_relay(&relay).await.map_err(internal)?;

    ok(())
}

pub async fn reactivate_relay(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = api.get_relay_or_404(&id).await?;
    api.require_admin_or_tenant(&auth, &relay.tenant_pubkey)?;

    if relay.status == RELAY_STATUS_DELINQUENT {
        return Err(bad_request("relay-is-delinquent", "relay is delinquent"));
    }

    if relay.status == RELAY_STATUS_ACTIVE {
        return Err(bad_request("relay-is-active", "relay is already active"));
    }

    command::activate_relay(&relay).await.map_err(internal)?;

    ok(())
}

// --- helpers ----------------------------------------------------------------

async fn fetch_relay_members(relay: &Relay) -> Result<Vec<String>> {
    if relay.synced == 0 {
        return Ok(Vec::new());
    }

    infra::list_relay_members(&relay.id).await
}

const RESERVED_SUBDOMAINS: [&str; 3] = ["api", "admin", "internal"];

static SUBDOMAIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]{1,61}[a-z0-9]$").unwrap());

/// Validate and normalize a relay before persistence: enforce the subdomain
/// format and reserved names, require an existing plan that permits any enabled
/// premium features, and coerce the boolean columns to 0/1.
fn prepare_relay(mut relay: Relay) -> Result<Relay, ApiError> {
    if !SUBDOMAIN_RE.is_match(&relay.subdomain)
        || RESERVED_SUBDOMAINS.contains(&relay.subdomain.as_str())
    {
        return Err(unprocessable("invalid-subdomain", "subdomain is invalid"));
    }

    let plan = query::get_plan(&relay.plan_id)
        .map_err(|_| unprocessable("invalid-plan", "plan not found"))?;

    if (!plan.blossom && relay.blossom_enabled == 1)
        || (!plan.livekit && relay.livekit_enabled == 1)
    {
        return Err(unprocessable(
            "premium-feature",
            "feature requires a paid plan",
        ));
    }

    relay.policy_public_read = parse_bool_default(relay.policy_public_read, 0);
    relay.policy_public_write = parse_bool_default(relay.policy_public_write, 0);
    relay.policy_public_join = parse_bool_default(relay.policy_public_join, 0);
    relay.policy_strip_signatures = parse_bool_default(relay.policy_strip_signatures, 0);
    relay.groups_enabled = parse_bool_default(relay.groups_enabled, 1);
    relay.management_enabled = parse_bool_default(relay.management_enabled, 1);
    relay.blossom_enabled = parse_bool_default(relay.blossom_enabled, 0);
    relay.livekit_enabled = parse_bool_default(relay.livekit_enabled, 0);
    relay.push_enabled = parse_bool_default(relay.push_enabled, 1);

    Ok(relay)
}

/// Normalize and validate a tenant custom domain. An empty string is allowed and
/// clears the domain. Rejects malformed hostnames and any domain under the
/// platform's relay domain, which belong in the subdomain field instead.
fn normalize_custom_domain(domain: &str) -> Result<String, ApiError> {
    let domain = domain.trim().to_lowercase();

    if !domain.is_empty() {
        if !domains::CUSTOM_DOMAIN_RE.is_match(&domain) {
            return Err(unprocessable(
                "invalid-domain",
                "domain must be a valid hostname (e.g. relay.example.com)",
            ));
        }

        let relay_domain = &env::get().relay_domain;
        if domain == *relay_domain || domain.ends_with(&format!(".{relay_domain}")) {
            return Err(unprocessable(
                "reserved-domain",
                "use the subdomain field for domains under the platform's relay domain",
            ));
        }
    }

    Ok(domain)
}

/// Translate a duplicate-subdomain write into a 422; anything else is a 500.
fn map_relay_write_error(e: anyhow::Error) -> ApiError {
    if matches!(map_unique_error(&e), Some("subdomain-exists")) {
        unprocessable("subdomain-exists", "subdomain already exists")
    } else {
        internal(e)
    }
}
