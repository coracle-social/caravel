use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::api::{Api, AuthedPubkey};
use crate::infra;
use crate::web::{ApiResult, bad_request, internal, not_found, ok};

#[derive(Deserialize)]
pub struct ListModerationQuery {
    pub since: Option<i64>,
    pub until: Option<i64>,
}

/// The most recently seen events and uploads across every relay within a window
/// of `seen_at`, newest first. Page backwards inside the window by moving
/// `until` down to the `seen_at` of the last item; items sharing a second can
/// repeat across pages, so callers dedupe.
pub async fn list_moderation(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Query(params): Query<ListModerationQuery>,
) -> ApiResult {
    api.require_admin(&auth)?;

    let page = infra::list_moderation(params.since, params.until)
        .await
        .map_err(internal)?;

    ok(page)
}

/// Serve one of a relay's uploaded blobs. Proxied rather than linked because a
/// relay serves its own blobs only to its members, which an admin reviewing
/// other people's relays isn't — so a direct link renders nothing. Returns the
/// raw bytes, not the usual `{data, code}` envelope.
pub async fn get_moderation_blob(
    State(api): State<Arc<Api>>,
    AuthedPubkey(auth): AuthedPubkey,
    Path((relay_id, sha256)): Path<(String, String)>,
) -> ApiResult {
    api.require_admin(&auth)?;

    // Both halves land in the upstream url, so neither may be able to steer it.
    // Existence is deliberately not checked against our own relay table: zooid
    // also hosts relays we never provisioned, and the feed lists their content,
    // so it — not us — decides what resolves.
    if relay_id.is_empty()
        || !relay_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(bad_request("invalid-relay", "relay id is invalid"));
    }

    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad_request("invalid-sha256", "sha256 is invalid"));
    }

    let Some((content_type, bytes)) = infra::get_relay_blob(&relay_id, &sha256)
        .await
        .map_err(internal)?
    else {
        return Err(not_found("blob not found"));
    };

    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            // Blobs are content-addressed, so a hit stays valid forever.
            (
                header::CACHE_CONTROL,
                "private, max-age=86400, immutable".to_string(),
            ),
        ],
        bytes,
    )
        .into_response())
}
