//! NIP-98: who signed an `Authorization` header, checked against what it was made
//! for.

use base64::Engine;
use coracle_lib::events::Event;

use crate::crypto::sha256_hex;
use crate::domain;
use crate::error::{Error, Result};
use crate::models::kinds;

/// How far a strict NIP-98 signature's `created_at` may be from now.
const MAX_SKEW: i64 = 60;

/// The signer of a NIP-98 `Authorization` header made for exactly this request:
/// its `u` tag is `url`, its `method` tag is `method`, and it was signed within a
/// minute of `now`.
pub fn verify_request_auth(header: &str, url: &str, method: &str, now: i64) -> Result<String> {
    let event = decode_http_auth(header)?;

    if !same_url(event.tags.value("u").unwrap_or_default(), url) {
        return Err(unauthorized("authorization url mismatch"));
    }

    if !event
        .tags
        .value("method")
        .is_some_and(|signed| signed.eq_ignore_ascii_case(method))
    {
        return Err(unauthorized("authorization method mismatch"));
    }

    if (event.created_at - now).abs() > MAX_SKEW {
        return Err(unauthorized("authorization is stale"));
    }

    Ok(event.pubkey.to_hex())
}

/// The signer of a NIP-98 header made for a request to a relay on `host` with
/// this body: its `u` tag names the host, its `payload` tag is the body's hash,
/// and it was signed within a minute of `now`.
pub fn verify_payload_auth(header: &str, host: &str, body: &[u8], now: i64) -> Result<String> {
    let event = decode_http_auth(header)?;

    let url = event.tags.value("u").unwrap_or_default();
    if domain::relays::url_host(url) != host {
        return Err(unauthorized("authorization url mismatch"));
    }

    if event.tags.value("payload") != Some(sha256_hex(body).as_str()) {
        return Err(unauthorized("authorization payload mismatch"));
    }

    if (event.created_at - now).abs() > MAX_SKEW {
        return Err(unauthorized("authorization is stale"));
    }

    Ok(event.pubkey.to_hex())
}

/// The verified event an `Authorization: Nostr <base64 event>` header carries, as
/// NIP-98 and Blossom send it, in standard or url-safe base64.
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

fn decode_http_auth(header: &str) -> Result<Event> {
    let event = decode_authorization(header)?;
    if event.kind != kinds::HTTP_AUTH {
        return Err(unauthorized("authorization has the wrong kind"));
    }
    Ok(event)
}

fn same_url(signed: &str, expected: &str) -> bool {
    signed.trim_end_matches('/') == expected.trim_end_matches('/')
}

fn unauthorized(reason: &str) -> Error {
    Error::Unauthorized(reason.to_string())
}
