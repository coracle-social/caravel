//! Routes for tenants managing their own account, relays and invoices. An admin
//! may call each of them on any tenant's behalf.

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use caravel_core::billing::{self, ledger};
use caravel_core::hosting::{self, RelaySettings};
use caravel_core::relays::{self, Relay, activity};
use caravel_core::tenants::{self, Tenant};
use caravel_core::{Core, Error};
use serde::{Deserialize, Serialize};

use crate::auth::Authed;
use crate::response::{ApiResult, created, ok};

/// A tenant as the API shows it, which reveals whether an NWC url is set but not
/// the url.
#[derive(Serialize)]
pub struct TenantView {
    pub pubkey: String,
    pub return_url: String,
    pub nwc_is_set: bool,
    pub nwc_error: Option<String>,
    pub stripe_error: Option<String>,
    pub created_at: i64,
    pub billing_anchor: Option<i64>,
    pub stripe_customer_id: String,
    pub stripe_payment_method_id: Option<String>,
    pub churned_at: Option<i64>,
    pub banned_at: Option<i64>,
}

impl From<Tenant> for TenantView {
    fn from(tenant: Tenant) -> Self {
        Self {
            nwc_is_set: !tenant.nwc_url.is_empty(),
            pubkey: tenant.pubkey,
            return_url: tenant.return_url,
            nwc_error: tenant.nwc_error,
            stripe_error: tenant.stripe_error,
            created_at: tenant.created_at,
            billing_anchor: tenant.billing_anchor,
            stripe_customer_id: tenant.stripe_customer_id,
            stripe_payment_method_id: tenant.stripe_payment_method_id,
            churned_at: tenant.churned_at,
            banned_at: tenant.banned_at,
        }
    }
}

pub(crate) async fn load_tenant(core: &Core, pubkey: &str) -> Result<Tenant, Error> {
    tenants::get_tenant(core.db.pool(), pubkey)
        .await?
        .ok_or(Error::NotFound("tenant not found"))
}

async fn load_relay(core: &Core, id: &str) -> Result<Relay, Error> {
    relays::get_relay(core.db.pool(), id)
        .await?
        .ok_or(Error::NotFound("relay not found"))
}

async fn load_invoice(core: &Core, id: &str) -> Result<ledger::Invoice, Error> {
    ledger::get_invoice(core.db.pool(), id)
        .await?
        .ok_or(Error::NotFound("invoice not found"))
}

// --- Tenants ---

#[derive(Deserialize)]
pub struct CreateTenant {
    /// The calling client's account page, where every link the robot DMs the
    /// tenant points.
    return_url: String,
}

/// Become a tenant. Idempotent: an existing tenant is returned as it is.
pub async fn create_tenant(
    State(core): State<Core>,
    Authed(actor): Authed,
    Json(body): Json<CreateTenant>,
) -> ApiResult {
    let tenant = hosting::ensure_tenant(&core, &actor.pubkey, &body.return_url).await?;
    ok(TenantView::from(tenant))
}

pub async fn get_tenant(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&pubkey)?;
    ok(TenantView::from(load_tenant(&core, &pubkey).await?))
}

#[derive(Deserialize)]
pub struct UpdateTenant {
    nwc_url: Option<String>,
}

pub async fn update_tenant(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
    Json(body): Json<UpdateTenant>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&pubkey)?;
    load_tenant(&core, &pubkey).await?;

    if let Some(nwc_url) = body.nwc_url {
        hosting::set_tenant_nwc_url(&core, &pubkey, &nwc_url).await?;
    }

    ok(TenantView::from(load_tenant(&core, &pubkey).await?))
}

pub async fn list_tenant_relays(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&pubkey)?;
    ok(relays::list_relays_for_tenant(core.db.pool(), &pubkey).await?)
}

pub async fn list_tenant_invoices(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&pubkey)?;
    ok(ledger::list_invoices_for_tenant(core.db.pool(), &pubkey).await?)
}

/// Bring the tenant's billing up to date without charging them, settling anything
/// they paid out of band.
pub async fn reconcile_tenant(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&pubkey)?;
    let tenant = load_tenant(&core, &pubkey).await?;

    billing::sync_stripe_customer(&core, &tenant).await;
    let tenant = load_tenant(&core, &pubkey).await?;
    billing::reconcile_subscription(&core, &tenant, false).await?;

    ok(TenantView::from(load_tenant(&core, &pubkey).await?))
}

/// An unsaved invoice for what the tenant owes so far this period, or null.
pub async fn get_draft_invoice(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&pubkey)?;
    let tenant = load_tenant(&core, &pubkey).await?;
    ok(billing::get_draft_invoice(&core, &tenant).await?)
}

/// The line items behind the draft invoice.
pub async fn list_draft_invoice_items(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&pubkey)?;
    ok(ledger::list_unbilled_invoice_items_for_tenant(core.db.pool(), &pubkey).await?)
}

#[derive(Deserialize)]
pub struct StripeSessionParams {
    return_url: Option<String>,
}

/// A Stripe billing portal for the tenant to manage their saved cards.
pub async fn create_stripe_session(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(pubkey): Path<String>,
    Query(params): Query<StripeSessionParams>,
) -> ApiResult {
    actor.require_pubkey(&pubkey)?;
    let tenant = load_tenant(&core, &pubkey).await?;
    let url = core
        .stripe
        .create_portal_session(&tenant.stripe_customer_id, params.return_url.as_deref())
        .await
        .map_err(Error::from)?;

    ok(serde_json::json!({ "url": url }))
}

// --- Relays ---

#[derive(Deserialize)]
pub struct CreateRelay {
    tenant_pubkey: String,
    #[serde(flatten)]
    settings: RelaySettings,
}

pub async fn create_relay(
    State(core): State<Core>,
    Authed(actor): Authed,
    Json(body): Json<CreateRelay>,
) -> ApiResult {
    actor.require_pubkey_or_admin(&body.tenant_pubkey)?;
    created(hosting::create_relay(&core, &body.tenant_pubkey, body.settings).await?)
}

pub async fn get_relay(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = load_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    ok(relay)
}

pub async fn update_relay(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
    Json(settings): Json<RelaySettings>,
) -> ApiResult {
    let relay = load_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    ok(hosting::update_relay(&core, relay, settings).await?)
}

pub async fn list_relay_members(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = load_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    let members = hosting::list_relay_members(&core, &relay).await?;
    ok(serde_json::json!({ "members": members }))
}

pub async fn list_relay_activity(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = load_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    let activity = activity::list_activity_for_resource(core.db.pool(), &id).await?;
    ok(serde_json::json!({ "activity": activity }))
}

pub async fn deactivate_relay(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = load_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    hosting::deactivate_relay(&core, &relay).await?;
    ok(())
}

pub async fn reactivate_relay(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = load_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    hosting::reactivate_relay(&core, &relay).await?;
    ok(())
}

/// Delete a relay's data for good. The row stays as a record.
pub async fn purge_relay(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = load_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    hosting::purge_relay(&core, &relay).await?;
    ok(())
}

/// The relay's stored events as a JSONL download.
pub async fn export_relay_data(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let relay = load_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    if relay.is_purged() {
        return Err(Error::conflict("relay-is-purged", "relay has been purged").into());
    }

    let stream = core
        .zooid
        .export_events(&relay.id)
        .await
        .map_err(Error::from)?
        .ok_or(Error::NotFound(
            "relay is not provisioned on the hosting backend",
        ))?;

    Ok((
        [
            (header::CONTENT_TYPE, "application/x-ndjson".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}.jsonl\"", relay.subdomain),
            ),
        ],
        Body::from_stream(stream),
    )
        .into_response())
}

/// Store a JSONL dump of events in the relay, reporting the lines it refused.
pub async fn import_relay_data(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
    body: Body,
) -> ApiResult {
    let relay = load_relay(&core, &id).await?;
    actor.require_pubkey_or_admin(&relay.tenant_pubkey)?;
    if relay.is_purged() {
        return Err(Error::conflict("relay-is-purged", "relay has been purged").into());
    }

    let result = core
        .zooid
        .import_events(&relay.id, body.into_data_stream())
        .await
        .map_err(Error::from)?
        .ok_or(Error::NotFound(
            "relay is not provisioned on the hosting backend",
        ))?;

    ok(result)
}

// --- Invoices ---

pub async fn get_invoice(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let invoice = load_invoice(&core, &id).await?;
    actor.require_pubkey_or_admin(&invoice.tenant_pubkey)?;
    ok(invoice)
}

pub async fn list_invoice_items(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let invoice = load_invoice(&core, &id).await?;
    actor.require_pubkey_or_admin(&invoice.tenant_pubkey)?;
    ok(ledger::list_invoice_items_for_invoice(core.db.pool(), &id).await?)
}

/// Collect an open invoice now, without the reminder DM a failure would send.
pub async fn reconcile_invoice(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let invoice = load_invoice(&core, &id).await?;
    actor.require_pubkey_or_admin(&invoice.tenant_pubkey)?;

    if !invoice.is_open() {
        return ok(invoice);
    }

    let tenant = load_tenant(&core, &invoice.tenant_pubkey).await?;
    billing::ensure_bolt11_for_invoice(&core, &invoice).await?;
    billing::reconcile_payments(&core, &tenant, &invoice, true, false).await?;

    ok(load_invoice(&core, &id).await?)
}

/// A payable Lightning invoice for an invoice.
pub async fn ensure_invoice_bolt11(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
) -> ApiResult {
    let invoice = load_invoice(&core, &id).await?;
    actor.require_pubkey_or_admin(&invoice.tenant_pubkey)?;
    ok(billing::ensure_bolt11_for_invoice(&core, &invoice).await?)
}

#[derive(Deserialize)]
pub struct CheckoutParams {
    /// Where Stripe sends the tenant after paying or canceling.
    return_url: String,
}

/// A hosted Stripe Checkout page for paying an invoice by card, which can clear a
/// 3D Secure challenge the off-session charge can't.
pub async fn ensure_invoice_checkout(
    State(core): State<Core>,
    Authed(actor): Authed,
    Path(id): Path<String>,
    Query(params): Query<CheckoutParams>,
) -> ApiResult {
    let invoice = load_invoice(&core, &id).await?;
    actor.require_pubkey_or_admin(&invoice.tenant_pubkey)?;
    let tenant = load_tenant(&core, &invoice.tenant_pubkey).await?;
    let checkout =
        billing::ensure_checkout_for_invoice(&core, &tenant, &invoice, &params.return_url).await?;
    ok(serde_json::json!({ "url": checkout.url }))
}
