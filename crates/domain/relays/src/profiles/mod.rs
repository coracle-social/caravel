//! NIP-11: the relay information document, and what anyone may see of a relay.

use serde_json::{Value, json};

/// The NIPs every relay supports.
const SUPPORTED_NIPS: [u16; 11] = [1, 9, 11, 40, 42, 43, 45, 50, 62, 70, 77];

/// What a relay states about itself in its information document. A relay's own
/// row decides these; the process serving it adds its limits.
#[derive(Clone, Debug, Default)]
pub struct RelayInfo {
    pub name: String,
    pub description: String,
    pub icon: String,
    /// The tenant who owns the relay.
    pub tenant_pubkey: String,
    /// The relay's own key.
    pub pubkey: String,
    pub anonymous_read: bool,
    pub public_write: bool,
    pub groups_enabled: bool,
    pub management_enabled: bool,
    pub push_enabled: bool,
    pub blossom_enabled: bool,
    /// What a membership costs in cents, and how many seconds it lasts, when the
    /// relay sells them.
    pub membership: Option<(i64, i64)>,
    /// Where memberships are paid for.
    pub payments_url: Option<String>,
}

/// What the process serving the relay allows, which the document states too.
#[derive(Clone, Copy, Debug, Default)]
pub struct Limits {
    /// The largest `limit` a filter may ask for.
    pub max_limit: usize,
    pub max_subscriptions: usize,
    pub max_message_size: usize,
}

/// The document, given what the relay says about itself and what the process
/// allows.
pub fn document(info: &RelayInfo, limits: &Limits, max_subid_length: usize) -> Value {
    let mut document = json!({
        "name": info.name,
        "description": info.description,
        "icon": info.icon,
        "pubkey": info.tenant_pubkey,
        "self": info.pubkey,
        "software": "https://gitea.coracle.social/coracle/caravel",
        "version": env!("CARGO_PKG_VERSION"),
        "supported_nips": supported_nips(info),
        "limitation": {
            "auth_required": !info.anonymous_read,
            "restricted_writes": !info.public_write,
            "payment_required": info.membership.is_some(),
            "created_at_upper_limit": 30,
            "max_limit": limits.max_limit,
            "max_subscriptions": limits.max_subscriptions,
            "max_message_length": limits.max_message_size,
            "max_subid_length": max_subid_length,
        },
    });

    if let Some((amount, period)) = info.membership {
        document["fees"] = json!({
            // A currency amount is in its minor units, as Flotilla reads it.
            "subscription": [{ "amount": amount, "unit": "USD", "period": period }],
        });
    }

    if let Some(payments_url) = &info.payments_url {
        document["payments_url"] = json!(payments_url);
    }

    document
}

/// Every relay's NIPs, and those of the features the relay has on.
fn supported_nips(info: &RelayInfo) -> Vec<Value> {
    let mut nips: Vec<Value> = SUPPORTED_NIPS.iter().map(|nip| json!(nip)).collect();
    if info.groups_enabled {
        nips.push(json!(29));
    }
    if info.management_enabled {
        nips.push(json!(86));
    }
    if info.push_enabled {
        nips.push(json!("9a"));
    }
    if info.blossom_enabled {
        for bud in ["BUD-00", "BUD-01", "BUD-02", "BUD-04", "BUD-06", "BUD-11"] {
            nips.push(json!(bud));
        }
    }
    nips
}
