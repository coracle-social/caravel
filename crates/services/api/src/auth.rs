//! Extractors that authenticate a request. A handler that takes one requires it;
//! a handler that takes none is public.

use axum::extract::FromRequestParts;
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use caravel_core::{Core, sessions};
use caravel_util::auth::Actor;
use caravel_util::error::Error;

use crate::response::ApiError;

/// The actor behind a session's bearer token.
pub struct Authed(pub Actor);

/// Proof that the request is from an admin of the platform.
pub struct Admin;

impl FromRequestParts<Core> for Authed {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, core: &Core) -> Result<Self, Self::Rejection> {
        let header = authorization(&parts.headers)?;
        if header.starts_with("Nostr ") {
            return Ok(Self(crate::legacy::authenticate(core, header)?));
        }
        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| Error::Unauthorized("authorization must be a bearer token".into()))?;
        let pubkey = sessions::find(core, token.trim())
            .await?
            .ok_or_else(|| Error::Unauthorized("session is invalid or expired".into()))?
            .pubkey;

        Ok(Self(Actor::new(&core.config, &pubkey)))
    }
}

impl FromRequestParts<Core> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, core: &Core) -> Result<Self, Self::Rejection> {
        let Authed(actor) = Authed::from_request_parts(parts, core).await?;
        actor.require_admin()?;
        Ok(Self)
    }
}

pub fn authorization(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError(Error::Unauthorized("missing authorization header".into())))
}
