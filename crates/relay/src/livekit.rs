//! Calls: LiveKit tokens for a room's members, and the webhook LiveKit reports
//! participants through, which meters call time and publishes who is in each call.

use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use caravel_core::{auth, calls};
use serde_json::json;

use crate::{Relays, Route};

/// CORS for calls: any origin may ask, with a NIP-98 header.
fn cors(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Authorization"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, OPTIONS"),
    );
    response
}

fn refuse(status: StatusCode, reason: impl Into<String>) -> Response {
    cors((status, reason.into()).into_response())
}

pub async fn preflight() -> Response {
    cors(StatusCode::NO_CONTENT.into_response())
}

/// Whether the relay has calls: 204 if so.
pub async fn support(Extension(route): Extension<Route>) -> Response {
    if route.instance.core().livekit.is_some() && route.instance.state().relay.livekit_enabled {
        cors(StatusCode::NO_CONTENT.into_response())
    } else {
        refuse(StatusCode::NOT_FOUND, "this relay has no calls")
    }
}

/// A token for a room's call, for a member of the relay who may see the room,
/// signed for with NIP-98 for exactly this url.
pub async fn token(
    Extension(route): Extension<Route>,
    Path(room_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let core = route.instance.core();
    let state = route.instance.state();
    let Some(livekit) = core
        .livekit
        .as_ref()
        .filter(|_| state.relay.livekit_enabled)
    else {
        return refuse(StatusCode::NOT_FOUND, "this relay has no calls");
    };

    let now = core.clock.now();
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let path = format!("{}/.well-known/nip29/livekit/{room_id}", route.host);
    let pubkey =
        match auth::verify_request_auth(authorization, &format!("https://{path}"), "GET", now)
            .or_else(|_| {
                auth::verify_request_auth(authorization, &format!("http://{path}"), "GET", now)
            }) {
            Ok(pubkey) => pubkey,
            Err(error) => return refuse(StatusCode::UNAUTHORIZED, error.to_string()),
        };

    if !state.snapshot.has_access(&pubkey) {
        return refuse(StatusCode::FORBIDDEN, "not a member of this relay");
    }
    let Some(room) = state.snapshot.rooms.get(&room_id) else {
        return refuse(StatusCode::NOT_FOUND, "group not found");
    };
    if !room.livekit {
        return refuse(StatusCode::FORBIDDEN, "livekit not enabled for this group");
    }
    if room.restricted && !state.snapshot.has_room_access(room, &pubkey) {
        return refuse(StatusCode::FORBIDDEN, "not a group member");
    }

    let name = calls::livekit_room(&state.relay.id, &room_id);
    if let Err(error) = livekit.ensure_room(&name, now).await {
        tracing::warn!(relay = %state.relay.id, error = %error, "failed to create a call");
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, "failed to create room");
    }

    let identity = format!("{pubkey}:{}", hex::encode(rand::random::<[u8; 8]>()));
    match livekit.participant_token(&name, &identity, now) {
        Ok(token) => cors(
            axum::Json(json!({
                "server_url": livekit.url(),
                "participant_token": token,
            }))
            .into_response(),
        ),
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "failed to sign a token"),
    }
}

/// LiveKit's webhook, for every relay's calls: each room it names leads with the
/// relay it belongs to. A participant leaving credits their time to the relay, and
/// any change to who is in a call is published as the room's participant list.
pub async fn webhook(State(relays): State<Relays>, headers: HeaderMap, body: Bytes) -> Response {
    let core = &relays.inner.core;
    let Some(livekit) = core.livekit.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let event = match livekit.receive_webhook(authorization, &body) {
        Ok(event) => event,
        Err(error) => {
            return (
                StatusCode::UNAUTHORIZED,
                format!("invalid webhook: {error}"),
            )
                .into_response();
        }
    };

    let Some(name) = event.room.as_ref().map(|room| room.name.clone()) else {
        return (StatusCode::BAD_REQUEST, "missing room").into_response();
    };
    let Some((relay_id, room_id)) = calls::split_livekit_room(&name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(instance) = relays.instance(relay_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let state = instance.state();
    let Some(room) = state.snapshot.rooms.get(room_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !room.livekit {
        return (StatusCode::FORBIDDEN, "livekit not enabled for this group").into_response();
    }

    let participants = match event.event.as_str() {
        "room_finished" => Vec::new(),
        "participant_joined" | "participant_left" | "participant_connection_aborted" => {
            // The departure is metered first, so the clock doesn't depend on LiveKit
            // answering the participant list.
            if event.event != "participant_joined"
                && let Some(participant) = &event.participant
                && participant.joined_at > 0
                && event.created_at > participant.joined_at
            {
                let seconds = event.created_at - participant.joined_at;
                if let Err(error) = calls::record_session(
                    core.db.pool(),
                    relay_id,
                    &participant.sid,
                    room_id,
                    seconds,
                    event.created_at,
                )
                .await
                {
                    tracing::warn!(relay = relay_id, error = %error, "failed to record a call session");
                }
            }

            match livekit.list_participants(&name, core.clock.now()).await {
                Ok(identities) => participant_pubkeys(&identities),
                Err(error) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("failed to fetch livekit participants: {error}"),
                    )
                        .into_response();
                }
            }
        }
        _ => return StatusCode::NO_CONTENT.into_response(),
    };

    match calls::publish_participants(core, &state.key, relay_id, room_id, &participants).await {
        Ok(published) => {
            instance.fanout(&published);
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to update livekit participants: {error}"),
        )
            .into_response(),
    }
}

/// The distinct pubkeys behind participant identities, which are `<pubkey>:<nonce>`.
fn participant_pubkeys(identities: &[String]) -> Vec<String> {
    let mut pubkeys: Vec<String> = Vec::new();
    for identity in identities {
        let pubkey = identity
            .split(':')
            .next()
            .unwrap_or_default()
            .to_lowercase();
        if pubkey.len() == 64
            && pubkey.chars().all(|c| c.is_ascii_hexdigit())
            && !pubkeys.contains(&pubkey)
        {
            pubkeys.push(pubkey);
        }
    }
    pubkeys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn participants_are_named_once_by_pubkey() {
        let pubkey = "a".repeat(64);
        let identities = vec![
            format!("{pubkey}:one"),
            format!("{pubkey}:two"),
            "garbage".to_string(),
        ];
        assert_eq!(participant_pubkeys(&identities), vec![pubkey]);
    }
}
