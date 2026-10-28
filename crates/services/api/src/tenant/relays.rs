//! The relays a tenant owns: creating them, configuring them, and moving their data.

use axum::Json;
use axum::extract::{Path, State};
use caravel_billing::relays::RelaySettings;
use caravel_core::{Core, hosting, management, memberships, transfer};
use http_body_util::Limited;
use serde::Deserialize;

use crate::auth::Authed;
use axum::body::{Body, HttpBody};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;

use crate::response::{ApiResult, created, error, ok};

/// The largest import body taken, 1 GiB.
const MAX_IMPORT_SIZE: usize = 1 << 30;

#[derive(Deserialize)]
pub struct CreateRelay {
    tenant_pubkey: String,
    #[serde(flatten)]
    settings: RelaySettings,
}

pub async fn create_relay(
    State(core): State<Core>,
    Authed(actor): Authed,
    Json(body): Json<CreateRelay>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&body.tenant_pubkey)?;
    created(hosting::create_relay(&core, &body.tenant_pubkey, body.settings).await?)
}

pub async fn get_relay(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = hosting::require_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    ok(relay)
}

pub async fn update_relay(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
    Json(settings): Json<RelaySettings>,
) -> ApiResult {
    let relay = hosting::require_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    ok(hosting::update_relay(&core, relay, settings).await?)
}

pub async fn deactivate_relay(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = hosting::require_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    hosting::deactivate_relay(&core, &relay).await?;
    ok(())
}

pub async fn reactivate_relay(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = hosting::require_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    hosting::reactivate_relay(&core, &relay).await?;
    ok(())
}

/// Delete a relay's data for good, freeing its subdomain. The row stays as a record.
pub async fn purge_relay(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = hosting::require_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    hosting::purge_relay(&core, &relay).await?;
    ok(())
}

/// Erase a relay's events, media, rooms and access lists, keeping its settings.
pub async fn erase_relay(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = hosting::require_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    hosting::erase_relay(&core, &relay).await?;
    ok(())
}

pub async fn list_relay_activity(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = hosting::require_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    let activity = hosting::relay_activity(&core, &id).await?;
    ok(serde_json::json!({ "activity": activity }))
}

/// Make a pubkey a member of the relay.
pub async fn add_relay_member(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path((id, pubkey)): Path<(String, String)>,
) -> ApiResult {
    let relay = hosting::require_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    management::add_member(&core, &relay, &pubkey).await?;
    ok(())
}

/// Remove a member from the relay. Its owner can't be removed.
pub async fn remove_relay_member(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path((id, pubkey)): Path<(String, String)>,
) -> ApiResult {
    let relay = hosting::require_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    management::remove_member(&core, &relay, &pubkey).await?;
    ok(())
}

/// The relay's paid memberships, newest first.
pub async fn list_relay_memberships(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = hosting::require_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    ok(memberships::memberships_for_relay(&core, &relay.id).await?)
}

/// The relay's stored events as a JSONL download.
pub async fn export_relay_data(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = hosting::require_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    relay.require_unpurged()?;

    let stream = transfer::export_events(core.db.pool().clone(), relay.id.clone());

    Ok((
        [
            (header::CONTENT_TYPE, "application/x-ndjson".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}.jsonl\"", relay.subdomain),
            ),
        ],
        Body::from_stream(stream),
    )
        .into_response())
}

/// Store a JSONL dump of events in the relay, reporting the lines it refused. A
/// body over MAX_IMPORT_SIZE is refused, or broken off where it passes it.
pub async fn import_relay_data(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
    body: Body,
) -> ApiResult {
    let relay = hosting::require_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    relay.require_unpurged()?;

    if body.size_hint().lower() > MAX_IMPORT_SIZE as u64 {
        return Ok(error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "import-too-large",
            "an import may be at most 1 GiB",
        ));
    }
    let body = Body::new(Limited::new(body, MAX_IMPORT_SIZE));
    ok(transfer::import_events(&core, &relay, body.into_data_stream()).await?)
}
