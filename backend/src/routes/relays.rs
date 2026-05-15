use std::sync::Arc;

use anyhow::Result;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;

use crate::api::{Api, AuthedPubkey};
use crate::models::{
    RELAY_STATUS_ACTIVE, RELAY_STATUS_DELINQUENT, RELAY_STATUS_INACTIVE, Relay,
};
use crate::web::{ApiError, err, map_unique_error, ok, parse_bool_default};

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
) -> Result<Response, ApiError> {
    api.require_admin(&auth)?;

    match api.query.list_relays().await {
        Ok(relays) => Ok(ok(StatusCode::OK, relays)),
        Err(e) => Err(ApiError::Internal(e.to_string())),
    }
}

pub async fn get_relay(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let relay = match api.query.get_relay(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(err(StatusCode::NOT_FOUND, "not-found", "relay not found")),
        Err(e) => {
            return Ok(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            ));
        }
    };

    api.require_admin_or_tenant(&auth, &relay.tenant)?;

    Ok(ok(StatusCode::OK, relay))
}

pub async fn list_relay_activity(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let relay = match api.query.get_relay(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(err(StatusCode::NOT_FOUND, "not-found", "relay not found")),
        Err(e) => {
            return Ok(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            ));
        }
    };

    api.require_admin_or_tenant(&auth, &relay.tenant)?;

    match api.query.list_activity_for_relay(&id).await {
        Ok(activity) => Ok(ok(
            StatusCode::OK,
            serde_json::json!({ "activity": activity }),
        )),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

pub async fn list_relay_members(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let relay = match api.query.get_relay(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(err(StatusCode::NOT_FOUND, "not-found", "relay not found")),
        Err(e) => {
            return Ok(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            ));
        }
    };

    api.require_admin_or_tenant(&auth, &relay.tenant)?;

    match fetch_relay_members(&api, &relay).await {
        Ok(members) => Ok(ok(
            StatusCode::OK,
            serde_json::json!({ "members": members }),
        )),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

pub async fn create_relay(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Json(payload): Json<CreateRelayRequest>,
) -> Result<Response, ApiError> {
    api.require_admin_or_tenant(&auth, &payload.tenant)?;

    let relay_id = format!(
        "{}_{}",
        payload.subdomain.replace('-', "_"),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );

    let mut relay = Relay {
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

    relay = match prepare_relay(&api, relay) {
        Ok(r) => r,
        Err(e) => {
            return Ok(relay_validation_error_response(e));
        }
    };

    match api.command.create_relay(&relay).await {
        Ok(()) => Ok(ok(StatusCode::CREATED, relay)),
        Err(e) => {
            if matches!(map_unique_error(&e), Some("subdomain-exists")) {
                Ok(err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "subdomain-exists",
                    "subdomain already exists",
                ))
            } else {
                Ok(err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    &e.to_string(),
                ))
            }
        }
    }
}

pub async fn update_relay(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
    Json(payload): Json<UpdateRelayRequest>,
) -> Result<Response, ApiError> {
    let mut relay = match api.query.get_relay(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(err(StatusCode::NOT_FOUND, "not-found", "relay not found")),
        Err(e) => {
            return Ok(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            ));
        }
    };

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

    relay = match prepare_relay(&api, relay) {
        Ok(r) => r,
        Err(e) => {
            return Ok(relay_validation_error_response(e));
        }
    };

    let plan_changed = requested_plan
        .as_deref()
        .is_some_and(|requested| requested != current_plan);

    if plan_changed {
        let selected_plan = api
            .query
            .get_plan(&relay.plan)
            .expect("validated plan must exist");
        if let Some(limit) = selected_plan.members {
            let current_members = match fetch_relay_members(&api, &relay).await {
                Ok(members) => members.len() as i64,
                Err(e) => {
                    return Ok(err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal",
                        &e.to_string(),
                    ));
                }
            };

            if current_members > limit {
                let message = format!(
                    "relay has {current_members} members, which exceeds the {} plan limit of {limit}",
                    selected_plan.name.to_lowercase()
                );
                return Ok(err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "member-limit-exceeded",
                    &message,
                ));
            }
        }
    }

    match api.command.update_relay(&relay).await {
        Ok(()) => Ok(ok(StatusCode::OK, relay)),
        Err(e) => {
            if matches!(map_unique_error(&e), Some("subdomain-exists")) {
                Ok(err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "subdomain-exists",
                    "subdomain already exists",
                ))
            } else {
                Ok(err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    &e.to_string(),
                ))
            }
        }
    }
}

pub async fn deactivate_relay(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let relay = match api.query.get_relay(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(err(StatusCode::NOT_FOUND, "not-found", "relay not found")),
        Err(e) => {
            return Ok(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            ));
        }
    };

    api.require_admin_or_tenant(&auth, &relay.tenant)?;

    if relay.status == RELAY_STATUS_INACTIVE || relay.status == RELAY_STATUS_DELINQUENT {
        return Ok(err(
            StatusCode::BAD_REQUEST,
            "relay-is-inactive",
            "relay is already inactive",
        ));
    }

    match api.command.deactivate_relay(&relay).await {
        Ok(()) => Ok(ok(StatusCode::OK, ())),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
}

pub async fn reactivate_relay(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let relay = match api.query.get_relay(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(err(StatusCode::NOT_FOUND, "not-found", "relay not found")),
        Err(e) => {
            return Ok(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &e.to_string(),
            ));
        }
    };

    api.require_admin_or_tenant(&auth, &relay.tenant)?;

    if relay.status == RELAY_STATUS_ACTIVE {
        return Ok(err(
            StatusCode::BAD_REQUEST,
            "relay-is-active",
            "relay is already active",
        ));
    }

    match api.command.activate_relay(&relay).await {
        Ok(()) => Ok(ok(StatusCode::OK, ())),
        Err(e) => Ok(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        )),
    }
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

fn relay_validation_error_response(error: RelayValidationError) -> Response {
    err(
        StatusCode::UNPROCESSABLE_ENTITY,
        error.code(),
        error.message(),
    )
}
