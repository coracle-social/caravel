//! Serving NIP-11: the document the domains state, as a response.

use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use caravel_relays::profiles::{Limits, RelayInfo};
use serde_json::Value;

/// The document, given what the relay says about itself and what this process
/// allows.
pub fn document(info: &RelayInfo, limits: &Limits) -> Value {
    caravel_relays::profiles::document(info, limits, crate::message::MAX_SUBSCRIPTION_ID_LEN)
}

pub fn response(document: Value) -> Response {
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/nostr+json"),
        )],
        document.to_string(),
    )
        .into_response()
}
