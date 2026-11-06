//! The sessionless NIP-98 authorization clients from before sessions still send:
//! a signature over the legacy API url alone, reused for any request while it's
//! fresh. Delete this module, `LEGACY_API_URL` and the legacy mount once those
//! clients are gone.

use caravel_core::Core;
use caravel_util::auth::{Actor, decode_authorization};
use caravel_util::error::Error;
use caravel_util::nostr::kinds;

/// How far a legacy signature's `created_at` may be from now. Those clients
/// reuse one signature for ten minutes.
const MAX_AGE: i64 = 15 * 60;

/// The actor behind a legacy `Authorization: Nostr` header, if the platform still
/// takes them.
pub fn authenticate(core: &Core, header: &str) -> Result<Actor, Error> {
    let refuse = |reason: &str| Error::Unauthorized(reason.to_string());
    let Some(legacy_url) = &core.config.legacy_api_url else {
        return Err(refuse("authorization must be a bearer token"));
    };

    let event = decode_authorization(header)?;
    if event.kind != kinds::HTTP_AUTH {
        return Err(refuse("authorization has the wrong kind"));
    }
    let signed_url = event.tags.value("u").unwrap_or_default();
    if signed_url.trim_end_matches('/') != legacy_url.trim_end_matches('/') {
        return Err(refuse("authorization url mismatch"));
    }
    if (event.created_at - core.clock.now()).abs() > MAX_AGE {
        return Err(refuse("authorization is stale"));
    }

    let pubkey = event.pubkey.to_hex();
    tracing::info!(pubkey = %pubkey, "legacy authorization");
    Ok(Actor::new(&core.config, &pubkey))
}
