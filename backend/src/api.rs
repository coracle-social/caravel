//! HTTP surface: router assembly and authentication.
//!
//! This module owns the shared `Api`, the NIP-98 `AuthedPubkey`
//! extractor that decorates protected handlers, and a small set of
//! authorization helpers (`require_admin`, `require_admin_or_tenant`,
//! `get_tenant_or_404`) that route handlers in [`crate::routes`] call
//! once they have an authenticated pubkey.
//!
//! The route signature convention is:
//! - `State<Arc<Api>>` for shared services,
//! - `AuthedPubkey` if the route requires an authenticated caller,
//! - then path / query / body extractors.
//!
//! Authentication is enforced at the extractor boundary; authorization
//! (admin vs tenant ownership) is the handler's responsibility.

use std::sync::Arc;

use anyhow::{Result, anyhow, ensure};
use axum::{
    Router,
    async_trait,
    extract::FromRequestParts,
    http::{HeaderMap, request::Parts},
    routing::{get, post},
};
use base64::Engine;
use nostr_sdk::{Event, JsonUtil, Kind};

use crate::billing::Billing;
use crate::env;
use crate::models::{Relay, Tenant};
use crate::query;
use crate::robot::Robot;
use crate::stripe::Stripe;
use crate::routes::identity::get_identity;
use crate::routes::invoices::{get_invoice, get_invoice_bolt11, get_tenant_latest_invoice};
use crate::routes::plans::{get_plan, list_plans};
use crate::routes::relays::{
    create_relay, deactivate_relay, get_relay, list_relay_activity, list_relay_members,
    list_relays, reactivate_relay, update_relay,
};
use crate::routes::tenants::{
    create_stripe_session, create_tenant, get_tenant, list_tenant_invoices, list_tenant_relays,
    list_tenants, update_tenant,
};
use crate::web::{ApiError, forbidden, internal, not_found, unauthorized};

#[derive(Clone)]
pub struct Api {
    pub billing: Billing,
    pub stripe: Stripe,
    pub robot: Robot,
}

impl Api {
    pub fn new(billing: Billing, stripe: Stripe, robot: Robot) -> Self {
        Self {
            billing,
            stripe,
            robot,
        }
    }

    pub fn router(self) -> Router {
        let api = Arc::new(self);

        Router::new()
            .route("/identity", get(get_identity))
            .route("/plans", get(list_plans))
            .route("/plans/:id", get(get_plan))
            .route("/tenants", get(list_tenants).post(create_tenant))
            .route("/tenants/:pubkey", get(get_tenant).put(update_tenant))
            .route("/tenants/:pubkey/relays", get(list_tenant_relays))
            .route("/tenants/:pubkey/invoices", get(list_tenant_invoices))
            .route(
                "/tenants/:pubkey/invoices/latest",
                get(get_tenant_latest_invoice),
            )
            .route("/tenants/:pubkey/stripe/session", get(create_stripe_session))
            .route("/relays", get(list_relays).post(create_relay))
            .route("/relays/:id", get(get_relay).put(update_relay))
            .route("/relays/:id/members", get(list_relay_members))
            .route("/relays/:id/activity", get(list_relay_activity))
            .route("/relays/:id/deactivate", post(deactivate_relay))
            .route("/relays/:id/reactivate", post(reactivate_relay))
            .route("/invoices/:id", get(get_invoice))
            .route("/invoices/:id/bolt11", get(get_invoice_bolt11))
            .with_state(api)
    }

    // --- authorization helpers ----------------------------------------------

    pub fn is_admin(&self, pubkey: &str) -> bool {
        env::get().server_admin_pubkeys.iter().any(|a| a == pubkey)
    }

    pub fn require_admin(&self, authorized_pubkey: &str) -> Result<(), ApiError> {
        if self.is_admin(authorized_pubkey) {
            Ok(())
        } else {
            Err(forbidden("admin required"))
        }
    }

    pub fn require_tenant(
        &self,
        authorized_pubkey: &str,
        tenant_pubkey: &str,
    ) -> Result<(), ApiError> {
        if authorized_pubkey == tenant_pubkey {
            Ok(())
        } else {
            Err(forbidden("not authorized"))
        }
    }

    pub fn require_admin_or_tenant(
        &self,
        authorized_pubkey: &str,
        tenant_pubkey: &str,
    ) -> Result<(), ApiError> {
        if self.is_admin(authorized_pubkey) || authorized_pubkey == tenant_pubkey {
            Ok(())
        } else {
            Err(forbidden("not authorized"))
        }
    }

    pub async fn get_tenant_or_404(&self, pubkey: &str) -> Result<Tenant, ApiError> {
        match query::get_tenant(pubkey).await {
            Ok(Some(t)) => Ok(t),
            Ok(None) => Err(not_found("tenant not found")),
            Err(e) => Err(internal(e)),
        }
    }

    pub async fn get_relay_or_404(&self, id: &str) -> Result<Relay, ApiError> {
        match query::get_relay(id).await {
            Ok(Some(r)) => Ok(r),
            Ok(None) => Err(not_found("relay not found")),
            Err(e) => Err(internal(e)),
        }
    }

    // --- authentication -----------------------------------------------------

    /// Decode the NIP-98 `Authorization` header and return the signer pubkey.
    ///
    /// This is the intentional session-style variant of NIP-98 used by the
    /// Caravel API: it validates signer identity plus host affinity, and does
    /// not bind to exact request URL/method or maintain replay state. Any
    /// failure surfaces as a 401 response.
    fn extract_auth_pubkey(&self, headers: &HeaderMap) -> Result<String, ApiError> {
        self.decode_nip98_pubkey(headers).map_err(unauthorized)
    }

    fn decode_nip98_pubkey(&self, headers: &HeaderMap) -> Result<String> {
        let auth = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| anyhow!("missing authorization header"))?;
        ensure!(
            auth.starts_with("Nostr "),
            "authorization must use Nostr scheme"
        );

        let (_, b64) = auth
            .split_once(' ')
            .ok_or_else(|| anyhow!("malformed authorization header"))?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64)?;
        let json = String::from_utf8(bytes)?;
        let event = Event::from_json(json)?;

        ensure!(event.kind == Kind::HttpAuth, "invalid nip98 kind");
        event.verify()?;

        let got_u = event
            .tags
            .iter()
            .filter_map(|tag| {
                let values = tag.as_slice();
                (values.len() >= 2 && values[0] == "u").then(|| values[1].to_string())
            })
            .last()
            .ok_or_else(|| anyhow!("missing u tag"))?;

        ensure!(got_u == env::get().server_host, "authorization host mismatch");

        Ok(event.pubkey.to_hex())
    }
}

/// Axum extractor that authenticates a request via NIP-98 and yields the
/// signer's pubkey. Adding this parameter to a handler signature is the
/// decoration that enforces "must be authenticated"; handlers that omit it
/// remain anonymous.
pub struct AuthedPubkey(pub String);

#[async_trait]
impl FromRequestParts<Arc<Api>> for AuthedPubkey {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        api: &Arc<Api>,
    ) -> Result<Self, Self::Rejection> {
        let pubkey = api.extract_auth_pubkey(&parts.headers)?;
        Ok(Self(pubkey))
    }
}
