//! Reading the event a NIP-98 or Blossom `Authorization` header carries.

use base64::Engine;
use coracle_lib::events::Event;

use crate::error::{Error, Result};
use caravel_util::nostr::kinds;

pub fn decode_authorization(header: &str) -> Result<Event> {
    let encoded = header
        .strip_prefix("Nostr ")
        .ok_or_else(|| unauthorized("authorization must use the Nostr scheme"))?
        .trim();
    let json = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(encoded))
        .map_err(|_| unauthorized("authorization is not base64"))?;
    let event: Event =
        serde_json::from_slice(&json).map_err(|_| unauthorized("authorization is not an event"))?;

    event
        .verify()
        .map_err(|_| unauthorized("authorization signature is invalid"))?;

    Ok(event)
}

pub(super) fn decode_http_auth(header: &str) -> Result<Event> {
    let event = decode_authorization(header)?;
    if event.kind != kinds::HTTP_AUTH {
        return Err(unauthorized("authorization has the wrong kind"));
    }
    Ok(event)
}

pub(super) fn same_url(signed: &str, expected: &str) -> bool {
    signed.trim_end_matches('/') == expected.trim_end_matches('/')
}

pub(super) fn unauthorized(reason: &str) -> Error {
    Error::Unauthorized(reason.to_string())
}
