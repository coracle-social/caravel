//! NIP-86: managing a relay over HTTP. Each call carries a NIP-98 signature over
//! its own body, and runs only if its signer holds the method.

use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use caravel_core::bus::Change;
use caravel_core::management::{self, Call};
use coracle_lib::events::Event;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::Route;
use crate::auth::relay_host;

const HTTP_AUTH_KIND: u16 = 27235;

/// How far a call's signature may be from the relay's clock.
const MAX_SKEW: i64 = 60;

#[derive(Deserialize)]
struct Request {
    method: String,
    #[serde(default)]
    params: Vec<Value>,
}

/// Answer a management call: `{"result": …}`, or `{"error": …}`.
pub async fn handle(route: &Route, headers: &HeaderMap, body: &[u8]) -> Response {
    let state = route.instance.state();
    if !state.relay.management_enabled {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }

    let response = match call(route, headers, body).await {
        Ok(result) => json!({ "result": result }),
        Err(error) => json!({ "error": error }),
    };

    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/nostr+json+rpc"),
            ),
            (
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            ),
        ],
        response.to_string(),
    )
        .into_response()
}

async fn call(route: &Route, headers: &HeaderMap, body: &[u8]) -> Result<Value, String> {
    let instance = &route.instance;
    let core = instance.core();
    let state = instance.state();

    let pubkey = authenticate(headers, body, &route.host, core.clock.now())?;
    let request: Request =
        serde_json::from_slice(body).map_err(|_| "invalid json body".to_string())?;

    if !state.snapshot.may_call(&pubkey, &request.method) {
        return Err(if request.method == "supportedmethods" {
            "blocked: only relay admins can manage this relay.".to_string()
        } else {
            "blocked: you are not authorized to use this method".to_string()
        });
    }

    if request.method == "supportedmethods" {
        return Ok(json!(state.snapshot.methods_for(&pubkey)));
    }

    let call = Call::decode(&request.method, &request.params).map_err(|error| error.to_string())?;
    let changes_info = matches!(
        call,
        Call::ChangeRelayName(_) | Call::ChangeRelayDescription(_) | Call::ChangeRelayIcon(_)
    );
    let outcome = management::call(core, &state.relay, call)
        .await
        .map_err(|error| error.to_string())?;

    if changes_info {
        core.bus.publish(Change::RelayChanged {
            relay_id: state.relay.id.clone(),
        });
    } else if !request.method.starts_with("list") {
        instance.refresh().await;
        for event in &outcome.published {
            instance.fanout(event);
        }
    }

    Ok(outcome.result)
}

/// The signer of a NIP-98 header made for this relay's url and this body, within a
/// minute of now.
fn authenticate(headers: &HeaderMap, body: &[u8], host: &str, now: i64) -> Result<String, String> {
    let encoded = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Nostr "))
        .ok_or("blocked: please authenticate in order to manage this relay")?
        .trim();
    let json = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(encoded))
        .map_err(|_| "invalid base64 auth")?;
    let event: Event = serde_json::from_slice(&json).map_err(|_| "invalid auth event json")?;

    if event.kind != HTTP_AUTH_KIND || event.verify().is_err() {
        return Err("invalid auth event".to_string());
    }

    let url = event.tags.value("u").ok_or("missing \"u\" tag")?;
    if relay_host(url) != host {
        return Err(format!("invalid \"u\" tag, expected '{host}', got '{url}'"));
    }

    let payload = hex::encode(Sha256::digest(body));
    if event.tags.value("payload") != Some(payload.as_str()) {
        return Err("invalid auth event payload hash".to_string());
    }

    if (event.created_at - now).abs() > MAX_SKEW {
        return Err("auth event is too old".to_string());
    }

    Ok(event.pubkey.to_hex())
}
