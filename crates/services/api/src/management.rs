//! Managing a relay from the dashboards, for its tenant or a platform admin: its
//! NIP-86 methods, its NIP-43 members, its NIP-29 rooms, and the events it holds.

use axum::Json;
use axum::extract::{Path, Query, State};
use caravel_billing::relays::Relay;
use caravel_core::{Core, hosting, management};
use caravel_relays::events::EventPage;
use caravel_relays::management::Call;
use caravel_relays::management::MANAGED_METHODS;
use caravel_relays::rooms::{Room, RoomMember};
use caravel_util::auth::Actor;
use caravel_util::error::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::Authed;
use crate::response::{ApiResult, ok};

/// The most events one page of a relay's events holds.
const MAX_EVENTS: usize = 100;

/// A live relay its caller may manage.
async fn managed_relay(core: &Core, actor: &Actor, id: &str) -> Result<Relay, Error> {
    let relay = hosting::require_relay(core, id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    relay.require_unpurged()?;
    Ok(relay)
}

#[derive(Deserialize)]
pub struct MethodCall {
    method: String,
    #[serde(default)]
    params: Vec<Value>,
}

/// Carry out a NIP-86 method on the relay, as its owner would: every method it
/// supports, which `supportedmethods` lists.
pub async fn call_method(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
    Json(call): Json<MethodCall>,
) -> ApiResult {
    let relay = managed_relay(&core, &actor, &id).await?;
    if call.method == "supportedmethods" {
        return ok(MANAGED_METHODS);
    }

    ok(management::call(&core, &relay, Call::decode(&call.method, &call.params)?).await?)
}

/// The relay's members (NIP-43), with their roles and how each got in, and the
/// roles it defines.
pub async fn list_members(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = managed_relay(&core, &actor, &id).await?;
    let (members, roles) = management::access_lists(&core, &relay.id).await?;
    ok(serde_json::json!({
        "members": members,
        "roles": roles,
    }))
}

/// A room as the dashboards show it.
#[derive(Serialize)]
struct RoomView {
    #[serde(flatten)]
    room: Room,
    members: Vec<RoomMember>,
    invites: Vec<String>,
}

/// The relay's rooms (NIP-29), each with its members and invite codes.
pub async fn list_rooms(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = managed_relay(&core, &actor, &id).await?;
    let (rooms, members, invites) = management::rooms(&core, &relay.id).await?;

    let views: Vec<RoomView> = rooms
        .into_iter()
        .map(|room| RoomView {
            members: members
                .iter()
                .filter(|member| member.room_id == room.id)
                .cloned()
                .collect(),
            invites: invites
                .iter()
                .filter(|(room_id, _)| *room_id == room.id)
                .map(|(_, code)| code.clone())
                .collect(),
            room,
        })
        .collect();
    ok(views)
}

#[derive(Deserialize)]
pub struct Moderation {
    kind: u16,
    tags: Vec<Vec<String>>,
}

/// Publish a NIP-29 moderation event about one of the relay's rooms, which the
/// relay signs on the caller's behalf.
pub async fn moderate_room(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
    Json(moderation): Json<Moderation>,
) -> ApiResult {
    let relay = managed_relay(&core, &actor, &id).await?;
    management::moderate_room(&core, &relay, moderation.kind, moderation.tags).await?;
    ok(())
}

#[derive(Deserialize)]
pub struct EventQuery {
    /// Comma-separated kinds.
    kinds: Option<String>,
    /// Comma-separated hex ids.
    ids: Option<String>,
    until: Option<i64>,
    after: Option<String>,
    #[serde(default)]
    exclude_own: bool,
    limit: Option<usize>,
}

/// A page of the relay's stored events, at most a hundred.
pub async fn list_events(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
    Query(query): Query<EventQuery>,
) -> ApiResult {
    let relay = managed_relay(&core, &actor, &id).await?;
    let list = |value: &Option<String>| -> Vec<String> {
        value
            .iter()
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect()
    };
    let kinds = list(&query.kinds)
        .iter()
        .map(|kind| kind.parse::<u16>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| Error::invalid("invalid-kinds", "kinds must be numbers"))?;
    let page = EventPage {
        kinds,
        ids: list(&query.ids),
        until: query.until,
        after: query.after,
        exclude_own: query.exclude_own,
        limit: query.limit.unwrap_or(MAX_EVENTS).min(MAX_EVENTS),
    };

    let stored = management::page_events(&core, &relay, &page).await?;
    let events = stored
        .iter()
        .map(|event| serde_json::from_str::<Value>(&event.to_json(false)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| Error::Internal(error.into()))?;
    ok(events)
}
