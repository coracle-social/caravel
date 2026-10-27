//! Routes only platform admins may call.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use caravel_core::crypto::is_hex64;
use caravel_core::{Core, Error, domain, hosting, queries, sessions};
use serde::Deserialize;

use crate::auth::Admin;
use crate::response::{ApiResult, ok};
use crate::tenant::TenantView;

pub async fn list_tenants(State(core): State<Core>, _: Admin) -> ApiResult {
    let tenants = queries::tenants::list_tenants(core.db.pool()).await?;
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
    hosting::ban_tenant(&core, &pubkey).await?;
    ok(TenantView::from(
        queries::tenants::require_tenant(core.db.pool(), &pubkey).await?,
    ))
}

/// Lift a tenant's ban. Their purged relays stay purged.
pub async fn unban_tenant(
    State(core): State<Core>,
    _: Admin,
    Path(pubkey): Path<String>,
) -> ApiResult {
    hosting::unban_tenant(&core, &pubkey).await?;
    ok(TenantView::from(
        queries::tenants::require_tenant(core.db.pool(), &pubkey).await?,
    ))
}

/// Delete a tenant without banning them: forgive what they owe, purge their relays.
pub async fn delete_tenant(
    State(core): State<Core>,
    _: Admin,
    Path(pubkey): Path<String>,
) -> ApiResult {
    hosting::delete_tenant(&core, &pubkey).await?;
    ok(TenantView::from(
        queries::tenants::require_tenant(core.db.pool(), &pubkey).await?,
    ))
}

/// End every session a pubkey holds.
pub async fn revoke_sessions(
    State(core): State<Core>,
    _: Admin,
    Path(pubkey): Path<String>,
) -> ApiResult {
    let revoked = sessions::revoke(&core, &pubkey).await?;
    ok(serde_json::json!({ "revoked": revoked }))
}

pub async fn list_relays(State(core): State<Core>, _: Admin) -> ApiResult {
    ok(queries::relays::list_relays(core.db.pool()).await?)
}

pub async fn list_invoices(State(core): State<Core>, _: Admin) -> ApiResult {
    ok(queries::ledger::list_invoices(core.db.pool()).await?)
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
    ok(queries::moderation::list_moderation(
        core.db.pool(),
        &core.config.admin_pubkeys,
        window.since,
        window.until,
    )
    .await?)
}

/// One of a relay's blobs, raw rather than in the envelope, for admins reviewing
/// relays that keep their media to members.
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
        return Err(Error::invalid("invalid-relay", "relay id is invalid").into());
    }

    if !is_hex64(&sha256) {
        return Err(Error::invalid("invalid-sha256", "sha256 is invalid").into());
    }

    let blob = queries::media::get_blob(core.db.pool(), &relay_id, &sha256)
        .await?
        .ok_or(Error::NotFound("blob not found"))?;
    let read = core
        .blobs
        .get(&domain::media::blob_key(&relay_id, &sha256), None)
        .await
        .map_err(Error::from)?
        .ok_or(Error::NotFound("blob not found"))?;

    Ok((
        [
            (header::CONTENT_TYPE, blob.content_type),
            (
                header::CACHE_CONTROL,
                "private, max-age=86400, immutable".to_string(),
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; sandbox".to_string(),
            ),
        ],
        Body::from_stream(read.stream),
    )
        .into_response())
}
