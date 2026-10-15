//! The headless JSON API, for the platform's own dashboards and for any other
//! client, such as Flotilla. Handlers load what a request names, authorize the
//! actor against it with the core's rules, then call the core.

pub mod admin;
pub mod auth;
pub mod public;
pub mod response;
pub mod tenant;

use axum::Router;
use axum::routing::{delete, get, post, put};
use caravel_core::Core;

/// Every API route, served at the root of whatever path the app mounts it on.
pub fn router(core: Core) -> Router {
    Router::new()
        .route("/plans", get(public::list_plans))
        .route("/plans/{id}", get(public::get_plan))
        .route("/domains/check", get(public::check_domain))
        .route(
            "/session",
            post(public::create_session).delete(public::delete_session),
        )
        .route("/sessions/{pubkey}", delete(admin::revoke_sessions))
        .route("/identity", get(public::get_identity))
        .route(
            "/tenants",
            get(admin::list_tenants).post(tenant::create_tenant),
        )
        .route(
            "/tenants/{pubkey}",
            get(tenant::get_tenant).put(tenant::update_tenant),
        )
        .route("/tenants/{pubkey}/relays", get(tenant::list_tenant_relays))
        .route(
            "/tenants/{pubkey}/invoices",
            get(tenant::list_tenant_invoices),
        )
        .route(
            "/tenants/{pubkey}/invoices/draft",
            get(tenant::get_draft_invoice),
        )
        .route(
            "/tenants/{pubkey}/invoices/draft/items",
            get(tenant::list_draft_invoice_items),
        )
        .route(
            "/tenants/{pubkey}/reconcile",
            post(tenant::reconcile_tenant),
        )
        .route(
            "/tenants/{pubkey}/stripe/session",
            get(tenant::create_stripe_session),
        )
        .route("/tenants/{pubkey}/ban", post(admin::ban_tenant))
        .route("/tenants/{pubkey}/unban", post(admin::unban_tenant))
        .route(
            "/relays",
            get(admin::list_relays).post(tenant::create_relay),
        )
        .route(
            "/relays/{id}",
            get(tenant::get_relay).put(tenant::update_relay),
        )
        .route("/relays/{id}/members", get(tenant::list_relay_members))
        .route(
            "/relays/{id}/members/{pubkey}",
            put(tenant::add_relay_member).delete(tenant::remove_relay_member),
        )
        .route("/relays/{id}/activity", get(tenant::list_relay_activity))
        .route("/relays/{id}/deactivate", post(tenant::deactivate_relay))
        .route("/relays/{id}/reactivate", post(tenant::reactivate_relay))
        .route("/relays/{id}/purge", post(tenant::purge_relay))
        .route("/relays/{id}/export", get(tenant::export_relay_data))
        .route("/relays/{id}/import", post(tenant::import_relay_data))
        .route("/invoices", get(admin::list_invoices))
        .route("/invoices/{id}", get(tenant::get_invoice))
        .route("/invoices/{id}/items", get(tenant::list_invoice_items))
        .route("/invoices/{id}/reconcile", post(tenant::reconcile_invoice))
        .route("/invoices/{id}/bolt11", post(tenant::ensure_invoice_bolt11))
        .route(
            "/invoices/{id}/checkout",
            post(tenant::ensure_invoice_checkout),
        )
        .route("/moderation", get(admin::list_moderation))
        .route(
            "/moderation/blob/{relay_id}/{sha256}",
            get(admin::get_moderation_blob),
        )
        .with_state(core)
}
