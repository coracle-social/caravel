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
use axum::routing::{MethodRouter, delete, get, post, put};
use caravel_core::Core;
use serde::Deserialize;

/// Where a request asks for its caller to be sent back to, if anywhere.
#[derive(Deserialize)]
struct ReturnTo {
    return_url: Option<String>,
}

/// Every API route, served at the root of whatever path the app mounts it on.
pub fn router(core: Core) -> Router {
    routes()
        .into_iter()
        .fold(Router::new(), |router, (path, route)| {
            router.route(path, route)
        })
        .with_state(core)
}

/// Each API route's path and its handlers by method, public so tests can check every route.
pub fn routes() -> Vec<(&'static str, MethodRouter<Core>)> {
    vec![
        ("/plans", get(public::list_plans)),
        ("/plans/{id}", get(public::get_plan)),
        ("/domains/check", get(public::check_domain)),
        (
            "/session",
            post(public::create_session).delete(public::delete_session),
        ),
        ("/sessions/{pubkey}", delete(admin::revoke_sessions)),
        ("/identity", get(public::get_identity)),
        (
            "/tenants",
            get(admin::list_tenants).post(tenant::create_tenant),
        ),
        (
            "/tenants/{pubkey}",
            get(tenant::get_tenant).put(tenant::update_tenant),
        ),
        ("/tenants/{pubkey}/relays", get(tenant::list_tenant_relays)),
        (
            "/tenants/{pubkey}/invoices",
            get(tenant::list_tenant_invoices),
        ),
        (
            "/tenants/{pubkey}/invoices/draft",
            get(tenant::get_draft_invoice),
        ),
        (
            "/tenants/{pubkey}/invoices/draft/items",
            get(tenant::list_draft_invoice_items),
        ),
        (
            "/tenants/{pubkey}/reconcile",
            post(tenant::reconcile_tenant),
        ),
        (
            "/tenants/{pubkey}/stripe/session",
            get(tenant::create_stripe_session),
        ),
        ("/tenants/{pubkey}/payouts", get(tenant::get_payouts)),
        (
            "/tenants/{pubkey}/payouts/onboard",
            post(tenant::onboard_payouts),
        ),
        (
            "/tenants/{pubkey}/payouts/dashboard",
            post(tenant::open_payouts_dashboard),
        ),
        ("/tenants/{pubkey}/ban", post(admin::ban_tenant)),
        ("/tenants/{pubkey}/unban", post(admin::unban_tenant)),
        ("/tenants/{pubkey}/delete", post(admin::delete_tenant)),
        (
            "/relays",
            get(admin::list_relays).post(tenant::create_relay),
        ),
        (
            "/relays/{id}",
            get(tenant::get_relay).put(tenant::update_relay),
        ),
        ("/relays/{id}/members", get(management::list_members)),
        ("/relays/{id}/management", post(management::call_method)),
        ("/relays/{id}/rooms", get(management::list_rooms)),
        (
            "/relays/{id}/rooms/moderate",
            post(management::moderate_room),
        ),
        ("/relays/{id}/events", get(management::list_events)),
        (
            "/relays/{id}/members/{pubkey}",
            put(tenant::add_relay_member).delete(tenant::remove_relay_member),
        ),
        (
            "/relays/{id}/memberships",
            get(tenant::list_relay_memberships),
        ),
        ("/relays/{id}/offer", get(member::get_offer)),
        ("/relays/{id}/membership", get(member::get_membership)),
        (
            "/relays/{id}/membership/checkout",
            post(member::create_checkout),
        ),
        (
            "/relays/{id}/membership/reconcile",
            post(member::reconcile_checkout),
        ),
        (
            "/relays/{id}/membership/cancel",
            post(member::cancel_renewal),
        ),
        ("/relays/{id}/invites", post(member::create_invite)),
        ("/invites/{code}", get(member::get_invite)),
        ("/invites/{code}/reconcile", post(member::reconcile_invite)),
        ("/account", get(member::get_account)),
        ("/relays/{id}/activity", get(tenant::list_relay_activity)),
        ("/relays/{id}/deactivate", post(tenant::deactivate_relay)),
        ("/relays/{id}/reactivate", post(tenant::reactivate_relay)),
        ("/relays/{id}/purge", post(tenant::purge_relay)),
        ("/relays/{id}/erase", post(tenant::erase_relay)),
        ("/relays/{id}/export", get(tenant::export_relay_data)),
        ("/relays/{id}/import", post(tenant::import_relay_data)),
        ("/invoices", get(admin::list_invoices)),
        ("/invoices/{id}", get(tenant::get_invoice)),
        ("/invoices/{id}/items", get(tenant::list_invoice_items)),
        ("/invoices/{id}/reconcile", post(tenant::reconcile_invoice)),
        ("/invoices/{id}/bolt11", post(tenant::ensure_invoice_bolt11)),
        (
            "/invoices/{id}/checkout",
            post(tenant::ensure_invoice_checkout),
        ),
        ("/moderation", get(admin::list_moderation)),
        (
            "/moderation/blob/{relay_id}/{sha256}",
            get(admin::get_moderation_blob),
        ),
    ]
}
