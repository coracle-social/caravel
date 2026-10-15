//! Extractors that authenticate a request. A handler that takes one requires it;
//! a handler that takes none is public.

use axum::extract::FromRequestParts;
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use caravel_core::auth::{self, Actor};
use caravel_core::{Core, Error};

use crate::response::ApiError;

/// The actor behind a session's bearer token, or behind a NIP-98 header signed for
/// the API's base url, which clients from before sessions send on every request.
pub struct Authed(pub Actor);

/// An authenticated actor who administers the platform.
pub struct Admin(pub Actor);

impl FromRequestParts<Core> for Authed {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, core: &Core) -> Result<Self, Self::Rejection> {
        let header = authorization(&parts.headers)?;
        let now = core.clock.now();

        let pubkey = match header.strip_prefix("Bearer ") {
            Some(token) => {
                auth::find_session(core.db.pool(), token.trim(), now)
                    .await?
                    .ok_or_else(|| Error::Unauthorized("session is invalid or expired".into()))?
                    .pubkey
            }
            None => auth::verify_base_url_auth(header, &core.config.api_url, now)?,
        };

        Ok(Self(Actor::new(&core.config, &pubkey)))
    }
}

impl FromRequestParts<Core> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, core: &Core) -> Result<Self, Self::Rejection> {
        let Authed(actor) = Authed::from_request_parts(parts, core).await?;
        actor.require_admin()?;
        Ok(Self(actor))
    }
}

pub fn authorization(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError(Error::Unauthorized("missing authorization header".into())))
}
