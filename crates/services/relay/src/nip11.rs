//! NIP-11: the relay information document.

use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use caravel_relays::policy::CREATED_AT_UPPER_LIMIT;
use serde_json::{Value, json};

use crate::RelayLimits;
use crate::instance::InstanceState;
use crate::session::Transport;

/// The NIPs every relay supports.
const SUPPORTED_NIPS: [u16; 11] = [1, 9, 11, 40, 42, 43, 45, 50, 62, 70, 77];

/// What a relay supports: every relay's NIPs, and those of the features it has on.
/// Names that aren't numbers are given as strings.
fn supported_nips(relay: &caravel_billing::relays::Relay) -> Vec<Value> {
    let mut nips: Vec<Value> = SUPPORTED_NIPS.iter().map(|nip| json!(nip)).collect();
    if relay.groups_enabled {
        nips.push(json!(29));
    }
    if relay.management_enabled {
        nips.push(json!(86));
    }
    if relay.push_enabled {
        nips.push(json!("9a"));
    }
    if relay.blossom_enabled {
        for bud in ["BUD-00", "BUD-01", "BUD-02", "BUD-04", "BUD-06", "BUD-11"] {
            nips.push(json!(bud));
        }
    }
    nips
}

/// A relay's information document: its name and pubkeys, the rules a client meets
/// there, and what membership costs and where to pay for it.
pub fn document(state: &InstanceState, limits: &RelayLimits, transport: &Transport) -> Value {
    let relay = &state.relay;
    let paid = relay.charges_members();

    let mut document = json!({
        "name": relay.info_name,
        "description": relay.info_description,
        "icon": relay.info_icon,
        "pubkey": relay.tenant_pubkey,
        "self": relay.pubkey,
        "software": "https://gitea.coracle.social/coracle/caravel",
        "version": env!("CARGO_PKG_VERSION"),
        "supported_nips": supported_nips(relay),
        "limitation": {
            "auth_required": !relay.policy_anonymous_read,
            "restricted_writes": !relay.policy_public_write,
            "payment_required": paid,
            "created_at_upper_limit": CREATED_AT_UPPER_LIMIT,
            "max_limit": limits.max_limit,
            "max_subscriptions": limits.max_subscriptions,
            "max_message_length": transport.max_message_size,
            "max_subid_length": crate::message::MAX_SUBSCRIPTION_ID_LEN,
        },
    });

    if paid {
        document["payments_url"] = json!(relay.portal_url("/"));
        document["fees"] = json!({
            // A currency amount is in its minor units, as Flotilla reads it.
            "subscription": [{
                "amount": relay.member_price,
                "unit": "USD",
                "period": relay.member_interval.seconds(),
            }],
        });
    }

    document
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
