use std::sync::{Arc, LazyLock};

use anyhow::Result;
use axum::{
    Json,
    extract::{Path, State},
};
use regex::Regex;
use serde::Deserialize;

use crate::api::{Api, AuthedPubkey};
use crate::{command, query};
use crate::models::{
    RELAY_STATUS_ACTIVE, RELAY_STATUS_DELINQUENT, RELAY_STATUS_INACTIVE, Relay,
};
use crate::web::{
    ApiError, ApiResult, bad_request, created, internal, map_unique_error, ok,
    parse_bool_default, unprocessable,
};

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
    api.require_admin_or_tenant(&auth, &relay.tenant)?;
    ok(relay)
}

pub async fn list_relay_activity(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = api.get_relay_or_404(&id).await?;
    api.require_admin_or_tenant(&auth, &relay.tenant)?;

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
    api.require_admin_or_tenant(&auth, &relay.tenant)?;

    let members = fetch_relay_members(&api, &relay).await.map_err(internal)?;
    ok(serde_json::json!({ "members": members }))
}

#[derive(Deserialize)]
pub struct CreateRelayRequest {
    pub tenant: String,
    pub subdomain: String,
    pub plan: String,
    pub info_name: String,
    pub info_icon: String,
    pub info_description: String,
    pub policy_public_join: i64,
    pub policy_strip_signatures: i64,
    pub groups_enabled: i64,
    pub management_enabled: i64,
    pub blossom_enabled: i64,
    pub livekit_enabled: i64,
    pub push_enabled: i64,
}

pub async fn create_relay(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Json(payload): Json<CreateRelayRequest>,
) -> ApiResult {
    api.require_admin_or_tenant(&auth, &payload.tenant)?;

    let relay_id = format!(
        "{}_{}",
        payload.subdomain.replace('-', "_"),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );

    let relay = Relay {
        id: relay_id.clone(),
        tenant: payload.tenant,
        subdomain: payload.subdomain,
        plan: payload.plan,
        info_name: payload.info_name,
        info_icon: payload.info_icon,
        info_description: payload.info_description,
        policy_public_join: payload.policy_public_join,
        policy_strip_signatures: payload.policy_strip_signatures,
        groups_enabled: payload.groups_enabled,
        management_enabled: payload.management_enabled,
        blossom_enabled: payload.blossom_enabled,
        livekit_enabled: payload.livekit_enabled,
        push_enabled: payload.push_enabled,
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
    pub plan: Option<String>,
    pub info_name: Option<String>,
    pub info_icon: Option<String>,
    pub info_description: Option<String>,
    pub policy_public_join: Option<i64>,
    pub policy_strip_signatures: Option<i64>,
    pub groups_enabled: Option<i64>,
    pub management_enabled: Option<i64>,
    pub blossom_enabled: Option<i64>,
    pub livekit_enabled: Option<i64>,
    pub push_enabled: Option<i64>,
}

pub async fn update_relay(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
    Json(payload): Json<UpdateRelayRequest>,
) -> ApiResult {
    let mut relay = api.get_relay_or_404(&id).await?;

    api.require_admin_or_tenant(&auth, &relay.tenant)?;

    let current_plan = relay.plan.clone();
    let requested_plan = payload.plan.clone();

    if let Some(v) = payload.subdomain {
        relay.subdomain = v;
    }
    if let Some(v) = requested_plan.clone() {
        relay.plan = v;
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

    let relay = prepare_relay(relay)?;

    let plan_changed = requested_plan
        .as_deref()
        .is_some_and(|requested| requested != current_plan);

    if plan_changed {
        let selected_plan =
            query::get_plan(&relay.plan).expect("validated plan must exist");
        if let Some(limit) = selected_plan.members {
            let current_members = fetch_relay_members(&api, &relay)
                .await
                .map_err(internal)?
                .len() as i64;

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
    api.require_admin_or_tenant(&auth, &relay.tenant)?;

    if relay.status == RELAY_STATUS_DELINQUENT {
        return Err(bad_request("relay-is-delinquent", "relay is delinquent"));
    }

    if relay.status == RELAY_STATUS_INACTIVE {
        return Err(bad_request("relay-is-inactive", "relay is already inactive"));
    }

    command::deactivate_relay(&relay)
        .await
        .map_err(internal)?;

    ok(())
}

pub async fn reactivate_relay(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = api.get_relay_or_404(&id).await?;
    api.require_admin_or_tenant(&auth, &relay.tenant)?;

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

async fn fetch_relay_members(api: &Api, relay: &Relay) -> Result<Vec<String>> {
    if relay.synced == 0 {
        return Ok(Vec::new());
    }

    api.infra.list_relay_members(&relay.id).await
}

const RESERVED_SUBDOMAINS: [&str; 3] = ["api", "admin", "internal"];

static SUBDOMAIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]{1,61}[a-z0-9]$").unwrap());

fn prepare_relay(mut relay: Relay) -> Result<Relay, ApiError> {
    if !SUBDOMAIN_RE.is_match(&relay.subdomain)
        || RESERVED_SUBDOMAINS.contains(&relay.subdomain.as_str()) {
        return Err(unprocessable("invalid-subdomain", "subdomain is invalid"));
    }

    let plan = query::get_plan(&relay.plan)
        .ok_or_else(|| unprocessable("invalid-plan", "plan not found"))?;

    if (!plan.blossom && relay.blossom_enabled == 1) || (!plan.livekit && relay.livekit_enabled == 1) {
        return Err(unprocessable("premium-feature", "feature requires a paid plan"));
    }

    relay.policy_public_join = parse_bool_default(relay.policy_public_join, 0);
    relay.policy_strip_signatures = parse_bool_default(relay.policy_strip_signatures, 0);
    relay.groups_enabled = parse_bool_default(relay.groups_enabled, 1);
    relay.management_enabled = parse_bool_default(relay.management_enabled, 1);
    relay.blossom_enabled = parse_bool_default(relay.blossom_enabled, 0);
    relay.livekit_enabled = parse_bool_default(relay.livekit_enabled, 0);
    relay.push_enabled = parse_bool_default(relay.push_enabled, 1);

    Ok(relay)
}

fn map_relay_write_error(e: anyhow::Error) -> ApiError {
    if matches!(map_unique_error(&e), Some("subdomain-exists")) {
        unprocessable("subdomain-exists", "subdomain already exists")
    } else {
        internal(e)
    }
}
