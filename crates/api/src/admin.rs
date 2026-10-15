//! Routes only platform admins may call.

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use caravel_core::billing::ledger;
use caravel_core::{Core, Error, auth, hosting, relays, tenants};
use serde::Deserialize;

use crate::auth::Admin;
use crate::response::{ApiResult, ok};
use crate::tenant::{TenantView, load_tenant};

pub async fn list_tenants(State(core): State<Core>, _: Admin) -> ApiResult {
    let tenants = tenants::list_tenants(core.db.pool()).await?;
    ok(tenants
        .into_iter()
        .map(TenantView::from)
        .collect::<Vec<_>>())
}

/// Ban a tenant: block relay creation, forgive what they owe, purge their relays.
pub async fn ban_tenant(
    State(core): State<Core>,
    _: Admin,
    Path(pubkey): Path<String>,
) -> ApiResult {
    load_tenant(&core, &pubkey).await?;
    hosting::ban_tenant(&core, &pubkey).await?;
    ok(TenantView::from(load_tenant(&core, &pubkey).await?))
}

/// Lift a tenant's ban. Their purged relays stay purged.
pub async fn unban_tenant(
    State(core): State<Core>,
    _: Admin,
    Path(pubkey): Path<String>,
) -> ApiResult {
    hosting::unban_tenant(&core, &pubkey).await?;
    ok(TenantView::from(load_tenant(&core, &pubkey).await?))
}

/// End every session a pubkey holds.
pub async fn revoke_sessions(
    State(core): State<Core>,
    _: Admin,
    Path(pubkey): Path<String>,
) -> ApiResult {
    let revoked = auth::revoke_sessions(core.db.pool(), &pubkey).await?;
    ok(serde_json::json!({ "revoked": revoked }))
}

pub async fn list_relays(State(core): State<Core>, _: Admin) -> ApiResult {
    ok(relays::list_relays(core.db.pool()).await?)
}

pub async fn list_invoices(State(core): State<Core>, _: Admin) -> ApiResult {
    ok(ledger::list_invoices(core.db.pool()).await?)
}

#[derive(Deserialize)]
pub struct ModerationWindow {
    since: Option<i64>,
    until: Option<i64>,
}

/// The events and uploads every relay saw within a window of `seen_at`, newest
/// first. Page back by moving `until` to the last item's `seen_at`; items sharing
/// a second can repeat across pages.
pub async fn list_moderation(
    State(core): State<Core>,
    _: Admin,
    Query(window): Query<ModerationWindow>,
) -> ApiResult {
    let page = core
        .zooid
        .list_moderation(window.since, window.until)
        .await
        .map_err(Error::from)?;
    ok(page)
}

/// One of a relay's blobs, raw rather than in the envelope. Relays serve blobs to
/// their members only, so admins reviewing them read through here.
pub async fn get_moderation_blob(
    State(core): State<Core>,
    _: Admin,
    Path((relay_id, sha256)): Path<(String, String)>,
) -> ApiResult {
    if relay_id.is_empty()
        || !relay_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(Error::conflict("invalid-relay", "relay id is invalid").into());
    }

    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::conflict("invalid-sha256", "sha256 is invalid").into());
    }

    let (content_type, bytes) = core
        .zooid
        .get_blob(&relay_id, &sha256)
        .await
        .map_err(Error::from)?
        .ok_or(Error::NotFound("blob not found"))?;

    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CACHE_CONTROL,
                "private, max-age=86400, immutable".to_string(),
            ),
        ],
        bytes,
    )
        .into_response())
}
