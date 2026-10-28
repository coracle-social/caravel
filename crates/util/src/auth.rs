//! Who a request runs as, and NIP-98 authorizations.

use serde::Serialize;

use crate::config::Config;
use crate::error::{Error, Result};

/// The pubkey a request runs as, and whether it administers the platform.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Actor {
    pub pubkey: String,
    pub is_admin: bool,
}

impl Actor {
    pub fn new(config: &Config, pubkey: &str) -> Self {
        Self {
            pubkey: pubkey.to_string(),
            is_admin: config.is_admin(pubkey),
        }
    }

    pub fn require_admin(&self) -> Result<()> {
        if self.is_admin {
            Ok(())
        } else {
            Err(Error::Forbidden("admin required"))
        }
    }

    /// Allow the actor to act as `pubkey`: being it, or administering the platform.
    pub fn require_pubkey_or_admin(&self, pubkey: &str) -> Result<()> {
        if self.is_admin || self.pubkey == pubkey {
            Ok(())
        } else {
            Err(Error::Forbidden("not authorized"))
        }
    }

    pub fn require_pubkey(&self, pubkey: &str) -> Result<()> {
        if self.pubkey == pubkey {
            Ok(())
        } else {
            Err(Error::Forbidden("not authorized"))
        }
    }
}

use base64::Engine;
use coracle_lib::events::Event;

use crate::nostr::kinds;

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

use crate::encoding::sha256_hex;
use crate::urls::url_host;

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
    if url_host(url) != host {
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
