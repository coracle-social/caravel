//! The headless JSON API, for the platform's own dashboards and for any other
//! client, such as Flotilla. Handlers load what a request names, authorize the
//! actor against it with the core's rules, then call the core.

mod admin;
mod auth;
mod management;
mod member;
mod public;
mod response;
mod tenant;

use axum::Router;
use axum::routing::{delete, get, post, put};
use caravel_core::Core;
use serde::Deserialize;

/// Where a request asks for its caller to be sent back to, if anywhere.
#[derive(Deserialize)]
struct ReturnTo {
    return_url: Option<String>,
}

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
        .route("/tenants/{pubkey}/payouts", get(tenant::get_payouts))
        .route(
            "/tenants/{pubkey}/payouts/onboard",
            post(tenant::onboard_payouts),
        )
        .route(
            "/tenants/{pubkey}/payouts/dashboard",
            post(tenant::open_payouts_dashboard),
        )
        .route("/tenants/{pubkey}/ban", post(admin::ban_tenant))
        .route("/tenants/{pubkey}/unban", post(admin::unban_tenant))
        .route("/tenants/{pubkey}/delete", post(admin::delete_tenant))
        .route(
            "/relays",
            get(admin::list_relays).post(tenant::create_relay),
        )
        .route(
            "/relays/{id}",
            get(tenant::get_relay).put(tenant::update_relay),
        )
        .route("/relays/{id}/members", get(management::list_members))
        .route("/relays/{id}/management", post(management::call_method))
        .route("/relays/{id}/rooms", get(management::list_rooms))
        .route(
            "/relays/{id}/rooms/moderate",
            post(management::moderate_room),
        )
        .route("/relays/{id}/events", get(management::list_events))
        .route(
            "/relays/{id}/members/{pubkey}",
            put(tenant::add_relay_member).delete(tenant::remove_relay_member),
        )
        .route(
            "/relays/{id}/memberships",
            get(tenant::list_relay_memberships),
        )
        .route("/relays/{id}/offer", get(member::get_offer))
        .route("/relays/{id}/membership", get(member::get_membership))
        .route(
            "/relays/{id}/membership/checkout",
            post(member::create_checkout),
        )
        .route(
            "/relays/{id}/membership/reconcile",
            post(member::reconcile_checkout),
        )
        .route(
            "/relays/{id}/membership/cancel",
            post(member::cancel_renewal),
        )
        .route("/relays/{id}/invites", post(member::create_invite))
        .route("/invites/{code}", get(member::get_invite))
        .route("/invites/{code}/reconcile", post(member::reconcile_invite))
        .route("/account", get(member::get_account))
        .route("/relays/{id}/activity", get(tenant::list_relay_activity))
        .route("/relays/{id}/deactivate", post(tenant::deactivate_relay))
        .route("/relays/{id}/reactivate", post(tenant::reactivate_relay))
        .route("/relays/{id}/purge", post(tenant::purge_relay))
        .route("/relays/{id}/erase", post(tenant::erase_relay))
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
