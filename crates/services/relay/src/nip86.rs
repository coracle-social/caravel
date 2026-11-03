//! NIP-86: managing a relay over HTTP. Each call carries a NIP-98 signature over
//! its own body, and runs only if its signer holds the method.

use axum::body::Bytes;
use axum::extract::Extension;
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use caravel_core::management;
use caravel_relays::management::{Call, Outcome};
use coracle_lib::keys::PublicKey;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Route;

#[derive(Deserialize)]
struct Request {
    method: String,
    #[serde(default)]
    params: Vec<Value>,
}

/// Answer a management call, which arrives as a `POST` to the relay's url:
/// `{"result": …}`, or `{"error": …}`.
pub async fn handle(
    Extension(route): Extension<Route>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let state = route.instance.state();
    if !state.relay.management_enabled {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }

    let response = match call(&route, &headers, &body).await {
        Ok(result) => json!({ "result": result }),
        Err(error) => json!({ "error": error }),
    };

    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/nostr+json+rpc"),
        )],
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
    let outcome = management::run_call(core, &state.relay, call)
        .await
        .map_err(|error| error.to_string())?;

    if let Outcome::AccessChanged(published) = &outcome {
        instance.apply_access_change(published).await;
    }

    Ok(outcome.result())
}

/// The signer of a NIP-98 header made for this relay's url and this body.
fn authenticate(
    headers: &HeaderMap,
    body: &[u8],
    host: &str,
    now: i64,
) -> Result<PublicKey, String> {
    let header = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or("blocked: please authenticate in order to manage this relay")?;
    caravel_util::auth::verify_payload_auth(header, host, body, now)
        .map_err(|error| error.to_string())
}
