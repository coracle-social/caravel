//! The routes members use: their memberships of relays that sell them, checkouts
//! and renewals, and buying an invite, which needs no account.

use axum::Json;
use axum::extract::{Path, State};
use caravel_core::{Core, memberships};
use caravel_db::queries;
use caravel_util::error::Error;
use caravel_domain::memberships::{Membership, MembershipStatus};
use caravel_domain::relays::Relay;
use serde::{Deserialize, Serialize};

use crate::ReturnTo;
use crate::auth::Authed;
use crate::response::{ApiResult, ok};

/// What a member sees of one of their memberships.
#[derive(Serialize)]
struct MembershipView {
    status: MembershipStatus,
    current_period_end: Option<i64>,
}

impl From<&Membership> for MembershipView {
    fn from(membership: &Membership) -> Self {
        Self {
            status: membership.status,
            current_period_end: membership.current_period_end,
        }
    }
}

/// A relay that sells memberships, or a 404.
async fn load_selling_relay(core: &Core, id: &str) -> Result<Relay, Error> {
    queries::relays::get_relay(core.db.pool(), id)
        .await?
        .filter(Relay::charges_members)
        .ok_or(Error::NotFound("relay not found"))
}

/// The Stripe Checkout session a payer came back from.
#[derive(Deserialize)]
pub struct ReconcileRequest {
    checkout: String,
}

/// Every live membership the caller holds, with what each relay offers.
pub async fn get_account(State(core): State<Core>, Authed(actor): Authed) -> ApiResult {
    let mut entries = Vec::new();
    for membership in
        queries::memberships::list_live_memberships_for_pubkey(core.db.pool(), &actor.pubkey)
            .await?
    {
        if let Some(relay) =
            queries::relays::get_relay(core.db.pool(), &membership.relay_id).await?
        {
            entries.push(serde_json::json!({
                "offer": memberships::get_offer(&core, &relay).await?,
                "membership": MembershipView::from(&membership),
            }));
        }
    }
    ok(serde_json::json!({ "pubkey": actor.pubkey, "memberships": entries }))
}

/// What a relay sells memberships for. Anyone may ask.
pub async fn get_offer(State(core): State<Core>, Path(id): Path<String>) -> ApiResult {
    let relay = load_selling_relay(&core, &id).await?;
    ok(memberships::get_offer(&core, &relay).await?)
}

/// The caller's membership of a relay, or null.
pub async fn get_membership(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = load_selling_relay(&core, &id).await?;
    let membership =
        queries::memberships::get_live_membership(core.db.pool(), &relay.id, &actor.pubkey).await?;
    ok(serde_json::json!({ "membership": membership.as_ref().map(MembershipView::from) }))
}

/// Open a checkout for the caller to join, or renew their membership of, a relay.
pub async fn create_checkout(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
    Json(request): Json<ReturnTo>,
) -> ApiResult {
    let relay = load_selling_relay(&core, &id).await?;
    let url =
        memberships::open_checkout(&core, &relay, &actor.pubkey, request.return_url.as_deref())
            .await?;
    ok(serde_json::json!({ "url": url }))
}

/// Settle a checkout the caller came back from, letting them in at once.
pub async fn reconcile_checkout(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
    Json(request): Json<ReconcileRequest>,
) -> ApiResult {
    let relay = load_selling_relay(&core, &id).await?;
    let membership =
        queries::memberships::get_live_membership(core.db.pool(), &relay.id, &actor.pubkey)
            .await?
            .ok_or(Error::NotFound("membership not found"))?;
    let paid = memberships::reconcile_checkout(&core, &membership, &request.checkout).await?;
    ok(serde_json::json!({ "paid": paid }))
}

/// Stop the caller's membership of a relay from renewing.
pub async fn cancel_renewal(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = load_selling_relay(&core, &id).await?;
    let membership = memberships::cancel_renewal(&core, &relay, &actor.pubkey).await?;
    ok(MembershipView::from(&membership))
}

/// Open a checkout buying an invite to a relay. Anyone may buy one.
pub async fn create_invite(
    State(core): State<Core>,
    Path(id): Path<String>,
    Json(request): Json<ReturnTo>,
) -> ApiResult {
    let relay = load_selling_relay(&core, &id).await?;
    let url =
        memberships::open_invite_checkout(&core, &relay, request.return_url.as_deref()).await?;
    ok(serde_json::json!({ "url": url }))
}

/// An invite, for whoever holds its code.
pub async fn get_invite(State(core): State<Core>, Path(code): Path<String>) -> ApiResult {
    ok(memberships::get_invite(&core, &code).await?)
}

/// Settle the checkout an invite's buyer came back from, so the invite works at
/// once.
pub async fn reconcile_invite(
    State(core): State<Core>,
    Path(code): Path<String>,
    Json(request): Json<ReconcileRequest>,
) -> ApiResult {
    let membership = queries::memberships::get_membership_by_invite(core.db.pool(), &code)
        .await?
        .ok_or(Error::NotFound("invite not found"))?;
    let paid = memberships::reconcile_checkout(&core, &membership, &request.checkout).await?;
    ok(serde_json::json!({ "paid": paid }))
}
