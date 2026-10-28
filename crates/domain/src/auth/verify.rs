//! NIP-98: who signed an `Authorization` header, checked against what it was made
//! for.

use super::decode::{decode_http_auth, same_url, unauthorized};
use crate as domain;
use crate::error::Result;
use caravel_util::encoding::sha256_hex;

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
