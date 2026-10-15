//! NIP-11: the relay information document.

use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use caravel_core::relay_policy::CREATED_AT_UPPER_LIMIT;
use serde_json::{Value, json};

use crate::RelayLimits;
use crate::instance::InstanceState;
use crate::session::Transport;

/// The NIPs every relay supports.
const SUPPORTED_NIPS: [u16; 9] = [1, 9, 11, 40, 42, 50, 62, 70, 77];

/// A relay's information document: its name and pubkeys, and the rules a client
/// meets there.
pub fn document(state: &InstanceState, limits: &RelayLimits, transport: &Transport) -> Value {
    let relay = &state.relay;

    json!({
        "name": relay.info_name,
        "description": relay.info_description,
        "icon": relay.info_icon,
        "pubkey": relay.tenant_pubkey,
        "self": relay.pubkey,
        "software": "https://gitea.coracle.social/coracle/caravel",
        "version": env!("CARGO_PKG_VERSION"),
        "supported_nips": SUPPORTED_NIPS,
        "limitation": {
            "auth_required": !relay.policy_anonymous_read,
            "restricted_writes": !relay.policy_public_write,
            "payment_required": false,
            "created_at_upper_limit": CREATED_AT_UPPER_LIMIT,
            "max_limit": limits.max_limit,
            "max_subscriptions": limits.max_subscriptions,
            "max_message_length": transport.max_message_size,
            "max_subid_length": crate::message::MAX_SUBSCRIPTION_ID_LEN,
        },
    })
}

pub fn response(document: Value) -> Response {
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/nostr+json"),
            ),
            (
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            ),
        ],
        document.to_string(),
    )
        .into_response()
}
