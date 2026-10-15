//! Who is calling: NIP-98 signatures, the session tokens a signature buys, and the
//! actor a request runs as.

use base64::Engine;
use coracle_lib::events::Event;
use serde::Serialize;
use sqlx::PgExecutor;

use crate::config::Config;
use crate::crypto::{random_token, sha256_hex};
use crate::error::{Error, Result};

/// How long a session lasts.
pub const SESSION_TTL: i64 = 30 * 24 * 60 * 60;

const HTTP_AUTH_KIND: u16 = 27235;

/// How far a strict NIP-98 signature's `created_at` may be from now.
const MAX_SKEW: i64 = 60;

/// How old a reused NIP-98 header signed for the API's base url may be. Clients
/// re-sign every ten minutes.
const BASE_URL_AUTH_MAX_AGE: i64 = 15 * 60;

/// How stale a session's `last_used_at` may get before a request refreshes it.
const TOUCH_INTERVAL: i64 = 60 * 60;

/// A sign-in, held as the hash of the bearer token that stands for it.
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct Session {
    pub pubkey: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub last_used_at: i64,
}

/// The pubkey a request runs as, and whether it administers the platform.
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// The signer of a NIP-98 header made for the API's base url rather than for one
/// request, which clients that predate sessions reuse for every call. It is good
/// for fifteen minutes from signing.
pub fn verify_base_url_auth(header: &str, base_url: &str, now: i64) -> Result<String> {
    let event = decode_http_auth(header)?;

    if !same_url(event.tags.value("u").unwrap_or_default(), base_url) {
        return Err(unauthorized("authorization host mismatch"));
    }

    if event.created_at > now + MAX_SKEW || now - event.created_at > BASE_URL_AUTH_MAX_AGE {
        return Err(unauthorized("authorization is stale"));
    }

    Ok(event.pubkey.to_hex())
}

fn decode_http_auth(header: &str) -> Result<Event> {
    let encoded = header
        .strip_prefix("Nostr ")
        .ok_or_else(|| unauthorized("authorization must use the Nostr scheme"))?;
    let json = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|_| unauthorized("authorization is not base64"))?;
    let event: Event =
        serde_json::from_slice(&json).map_err(|_| unauthorized("authorization is not an event"))?;

    if event.kind != HTTP_AUTH_KIND {
        return Err(unauthorized("authorization has the wrong kind"));
    }

    event
        .verify()
        .map_err(|_| unauthorized("authorization signature is invalid"))?;

    Ok(event)
}

fn same_url(signed: &str, expected: &str) -> bool {
    signed.trim_end_matches('/') == expected.trim_end_matches('/')
}

fn unauthorized(reason: &str) -> Error {
    Error::Unauthorized(reason.to_string())
}

/// Start a session for `pubkey`, returning the bearer token that stands for it.
/// Only the token's hash is stored.
pub async fn create_session(
    db: impl PgExecutor<'_>,
    pubkey: &str,
    now: i64,
) -> Result<(String, Session)> {
    let token = random_token();
    let session = sqlx::query_as::<_, Session>(
        "INSERT INTO session (token_hash, pubkey, created_at, expires_at, last_used_at)
         VALUES ($1, $2, $3, $4, $3)
         RETURNING pubkey, created_at, expires_at, last_used_at",
    )
    .bind(sha256_hex(token.as_bytes()))
    .bind(pubkey)
    .bind(now)
    .bind(now + SESSION_TTL)
    .fetch_one(db)
    .await?;

    Ok((token, session))
}

/// The live session a bearer token stands for, refreshing its `last_used_at` at
/// most once an hour.
pub async fn find_session(
    db: impl PgExecutor<'_> + Copy,
    token: &str,
    now: i64,
) -> Result<Option<Session>> {
    let token_hash = sha256_hex(token.as_bytes());
    let session = sqlx::query_as::<_, Session>(
        "SELECT pubkey, created_at, expires_at, last_used_at FROM session
         WHERE token_hash = $1 AND expires_at > $2",
    )
    .bind(&token_hash)
    .bind(now)
    .fetch_optional(db)
    .await?;

    if let Some(session) = &session
        && now - session.last_used_at > TOUCH_INTERVAL
    {
        sqlx::query("UPDATE session SET last_used_at = $1 WHERE token_hash = $2")
            .bind(now)
            .bind(&token_hash)
            .execute(db)
            .await?;
    }

    Ok(session)
}

pub async fn delete_session(db: impl PgExecutor<'_>, token: &str) -> Result<()> {
    sqlx::query("DELETE FROM session WHERE token_hash = $1")
        .bind(sha256_hex(token.as_bytes()))
        .execute(db)
        .await?;
    Ok(())
}

/// End every session `pubkey` holds.
pub async fn revoke_sessions(db: impl PgExecutor<'_>, pubkey: &str) -> Result<u64> {
    let result = sqlx::query("DELETE FROM session WHERE pubkey = $1")
        .bind(pubkey)
        .execute(db)
        .await?;
    Ok(result.rows_affected())
}

pub async fn delete_expired_sessions(db: impl PgExecutor<'_>, now: i64) -> Result<u64> {
    let result = sqlx::query("DELETE FROM session WHERE expires_at <= $1")
        .bind(now)
        .execute(db)
        .await?;
    Ok(result.rows_affected())
}
