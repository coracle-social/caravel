use std::sync::Arc;

use anyhow::Result;
use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;

use crate::api::{Api, AuthedPubkey};
use crate::models::{
    RELAY_STATUS_ACTIVE, RELAY_STATUS_DELINQUENT, RELAY_STATUS_INACTIVE, Relay,
};
use crate::web::{
    ApiError, ApiResult, bad_request, created, internal, map_unique_error, ok,
    parse_bool_default, unprocessable,
};

#[derive(Deserialize)]
pub struct CreateRelayRequest {
    pub tenant: String,
    pub subdomain: String,
    pub plan: String,
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

pub async fn list_relays(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
) -> ApiResult {
    api.require_admin(&auth)?;

    let relays = api.query.list_relays().await.map_err(internal)?;
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

    let activity = api
        .query
        .list_activity_for_relay(&id)
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
        schema: relay_id.clone(),
        subdomain: payload.subdomain,
        plan: payload.plan,
        stripe_subscription_item_id: None,
        status: RELAY_STATUS_ACTIVE.to_string(),
        sync_error: String::new(),
        info_name: payload.info_name.unwrap_or_default(),
        info_icon: payload.info_icon.unwrap_or_default(),
        info_description: payload.info_description.unwrap_or_default(),
        policy_public_join: payload.policy_public_join.unwrap_or(0),
        policy_strip_signatures: payload.policy_strip_signatures.unwrap_or(0),
        groups_enabled: payload.groups_enabled.unwrap_or(1),
        management_enabled: payload.management_enabled.unwrap_or(1),
        blossom_enabled: payload.blossom_enabled.unwrap_or(0),
        livekit_enabled: payload.livekit_enabled.unwrap_or(0),
        push_enabled: payload.push_enabled.unwrap_or(1),
        synced: 0,
    };

    let relay = prepare_relay(&api, relay).map_err(validation_error)?;

    api.command
        .create_relay(&relay)
        .await
        .map_err(map_relay_write_error)?;
    created(relay)
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

    let relay = prepare_relay(&api, relay).map_err(validation_error)?;

    let plan_changed = requested_plan
        .as_deref()
        .is_some_and(|requested| requested != current_plan);

    if plan_changed {
        let selected_plan = api
            .query
            .get_plan(&relay.plan)
            .expect("validated plan must exist");
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

    api.command
        .update_relay(&relay)
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

    if relay.status == RELAY_STATUS_INACTIVE || relay.status == RELAY_STATUS_DELINQUENT {
        return Err(bad_request("relay-is-inactive", "relay is already inactive"));
    }

    api.command
        .deactivate_relay(&relay)
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

    if relay.status == RELAY_STATUS_ACTIVE {
        return Err(bad_request("relay-is-active", "relay is already active"));
    }

    api.command.activate_relay(&relay).await.map_err(internal)?;
    ok(())
}

// --- helpers ----------------------------------------------------------------

async fn fetch_relay_members(api: &Api, relay: &Relay) -> Result<Vec<String>> {
    if relay.synced == 0 {
        return Ok(Vec::new());
    }

    api.infra.list_relay_members(&relay.id).await
}

/// Validate user-supplied fields and fill defaults for `policy_*` / feature
/// flags. Premium feature flags are clamped against the plan's entitlements.
fn prepare_relay(api: &Api, mut relay: Relay) -> Result<Relay, RelayValidationError> {
    validate_subdomain_label(&relay.subdomain)?;

    let plan = api
        .query
        .get_plan(&relay.plan)
        .ok_or(RelayValidationError::InvalidPlan)?;

    if !plan.blossom && relay.blossom_enabled == 1 {
        return Err(RelayValidationError::PremiumFeature);
    }
    if !plan.livekit && relay.livekit_enabled == 1 {
        return Err(RelayValidationError::PremiumFeature);
    }

    if relay.status.is_empty() {
        relay.status = RELAY_STATUS_ACTIVE.to_string();
    }
    relay.policy_public_join = parse_bool_default(relay.policy_public_join, 0);
    relay.policy_strip_signatures = parse_bool_default(relay.policy_strip_signatures, 0);
    relay.groups_enabled = parse_bool_default(relay.groups_enabled, 1);
    relay.management_enabled = parse_bool_default(relay.management_enabled, 1);
    relay.blossom_enabled =
        parse_bool_default(relay.blossom_enabled, if plan.blossom { 1 } else { 0 });
    relay.livekit_enabled =
        parse_bool_default(relay.livekit_enabled, if plan.livekit { 1 } else { 0 });
    relay.push_enabled = parse_bool_default(relay.push_enabled, 1);

    Ok(relay)
}

fn validation_error(error: RelayValidationError) -> ApiError {
    unprocessable(error.code(), error.message())
}

fn map_relay_write_error(e: anyhow::Error) -> ApiError {
    if matches!(map_unique_error(&e), Some("subdomain-exists")) {
        unprocessable("subdomain-exists", "subdomain already exists")
    } else {
        internal(e)
    }
}

const SUBDOMAIN_LABEL_MAX_LEN: usize = 63;
const RESERVED_SUBDOMAIN_LABELS: [&str; 2] = ["api", "admin"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubdomainValidationError {
    Empty,
    TooLong,
    Reserved,
    EdgeHyphen,
    InvalidCharacters,
}

impl SubdomainValidationError {
    fn code(self) -> &'static str {
        match self {
            Self::Empty => "subdomain-empty",
            Self::TooLong => "subdomain-too-long",
            Self::Reserved => "subdomain-reserved",
            Self::EdgeHyphen => "subdomain-invalid-hyphen",
            Self::InvalidCharacters => "subdomain-invalid-characters",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::Empty => "subdomain is required",
            Self::TooLong => "subdomain must be 63 characters or fewer",
            Self::Reserved => "subdomain is reserved",
            Self::EdgeHyphen => "subdomain cannot start or end with a hyphen",
            Self::InvalidCharacters => {
                "subdomain may only contain lowercase letters, numbers, and hyphens"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelayValidationError {
    InvalidPlan,
    PremiumFeature,
    Subdomain(SubdomainValidationError),
}

impl RelayValidationError {
    fn code(self) -> &'static str {
        match self {
            Self::InvalidPlan => "invalid-plan",
            Self::PremiumFeature => "premium-feature",
            Self::Subdomain(reason) => reason.code(),
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::InvalidPlan => "plan not found",
            Self::PremiumFeature => "feature requires a paid plan",
            Self::Subdomain(reason) => reason.message(),
        }
    }
}

impl From<SubdomainValidationError> for RelayValidationError {
    fn from(value: SubdomainValidationError) -> Self {
        Self::Subdomain(value)
    }
}

fn validate_subdomain_label(subdomain: &str) -> Result<(), SubdomainValidationError> {
    if subdomain.is_empty() {
        return Err(SubdomainValidationError::Empty);
    }
    if subdomain.len() > SUBDOMAIN_LABEL_MAX_LEN {
        return Err(SubdomainValidationError::TooLong);
    }
    if subdomain.starts_with('-') || subdomain.ends_with('-') {
        return Err(SubdomainValidationError::EdgeHyphen);
    }
    if RESERVED_SUBDOMAIN_LABELS.contains(&subdomain) {
        return Err(SubdomainValidationError::Reserved);
    }
    if !subdomain
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(SubdomainValidationError::InvalidCharacters);
    }

    Ok(())
}
