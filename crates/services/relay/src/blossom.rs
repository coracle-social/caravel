//! Blossom: media on a relay's own hosts. Members upload, mirror, list and delete
//! with a kind 24242 authorization; anyone downloads, unless the relay keeps its
//! media to members. Blob responses are hardened, since their content type is the
//! uploader's choice and they are served on the relay's own origin.

use std::ops::Range;

use axum::body::{Body, Bytes};
use axum::extract::{Extension, Path, Request};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use caravel_core::media;
use caravel_relays::media::{Blob, Deleted, MAX_BLOB_SIZE};
use caravel_util::encoding::{is_hex64, sha256_hex};
use caravel_util::nostr::kinds::BLOSSOM_AUTH;
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::Route;

/// How far ahead of the clock an authorization may be dated.
const MAX_DRIFT: i64 = 600;

/// The types a browser displays without running anything the uploader wrote;
/// everything else downloads.
const INLINE_TYPES: [&str; 19] = [
    "image/apng",
    "image/avif",
    "image/gif",
    "image/heic",
    "image/heif",
    "image/jpeg",
    "image/png",
    "image/webp",
    "audio/aac",
    "audio/mp4",
    "audio/mpeg",
    "audio/ogg",
    "audio/wav",
    "audio/webm",
    "video/mp4",
    "video/mpeg",
    "video/ogg",
    "video/quicktime",
    "video/webm",
];

/// Why a request was turned away: a status, and the reason BUD-01 puts in
/// `X-Reason`.
pub struct Refused(StatusCode, String);

impl Refused {
    fn new(status: StatusCode, reason: impl Into<String>) -> Self {
        Self(status, reason.into())
    }

    /// The relay's own failure, logged with its cause.
    fn failed(reason: &'static str, error: impl std::fmt::Display) -> Self {
        tracing::warn!(error = %error, "{reason}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, reason)
    }
}

impl IntoResponse for Refused {
    fn into_response(self) -> Response {
        let Refused(status, reason) = self;
        let mut response = (status, reason.clone()).into_response();
        if let Ok(value) = HeaderValue::from_str(&reason) {
            response.headers_mut().insert("x-reason", value);
        }
        response
    }
}

type Answer = Result<Response, Refused>;

/// The hash a path names: 64 hex characters, optionally with an extension.
pub fn parse_name(name: &str) -> Option<String> {
    let hash = name.split('.').next()?.to_ascii_lowercase();
    is_hex64(&hash).then_some(hash)
}

fn extension(content_type: &str) -> Option<&'static str> {
    Some(match content_type.split(';').next()?.trim() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/avif" => "avif",
        "image/svg+xml" => "svg",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        "audio/wav" => "wav",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        _ => return None,
    })
}

/// A blob as BUD-02 describes it, at its url on the relay's primary host.
fn descriptor(route: &Route, blob: &Blob) -> Value {
    let url = match extension(&blob.content_type) {
        Some(ext) => format!("https://{}/{}.{ext}", route.primary_host(), blob.sha256),
        None => format!("https://{}/{}", route.primary_host(), blob.sha256),
    };

    json!({
        "url": url,
        "sha256": blob.sha256,
        "size": blob.size,
        "type": blob.content_type,
        "uploaded": blob.created_at,
    })
}

/// Refuse unless the relay has media on.
fn require_media(route: &Route) -> Result<(), Refused> {
    if route.instance.state().relay.blossom_enabled {
        Ok(())
    } else {
        Err(Refused::new(
            StatusCode::NOT_FOUND,
            "this relay has no media server",
        ))
    }
}

/// Check an authorization for `verb`, returning its signer's pubkey and the
/// hashes it covers. One naming servers must name one of this relay's hosts
/// (BUD-11), and only the relay's members may authorize anything.
fn authorize(
    route: &Route,
    headers: &HeaderMap,
    verb: &str,
) -> Result<(String, Vec<String>), Refused> {
    let unauthorized = |reason: &str| Refused::new(StatusCode::UNAUTHORIZED, reason);
    let header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| unauthorized("an Authorization: Nostr header is required"))?;
    let event = caravel_util::auth::decode_authorization(header)
        .map_err(|error| unauthorized(&error.to_string()))?;
    let now = route.instance.core().clock.now();

    if event.kind != BLOSSOM_AUTH {
        return Err(unauthorized("the authorization must be kind 24242"));
    }
    if event.created_at > now + MAX_DRIFT {
        return Err(unauthorized("the authorization is dated in the future"));
    }
    match coracle_lib::expiration::get_expiration(&event.tags) {
        Some(expiration) if expiration > now => {}
        Some(_) => return Err(unauthorized("the authorization has expired")),
        None => return Err(unauthorized("the authorization needs an expiration tag")),
    }
    if event.tags.value("t") != Some(verb) {
        return Err(unauthorized(&format!(
            "the authorization is not for {verb}"
        )));
    }
    let servers: Vec<&str> = event.tags.values("server").collect();
    if !servers.is_empty()
        && !route
            .instance
            .state()
            .relay
            .hosts()
            .iter()
            .any(|host| servers.contains(&host.as_str()))
    {
        return Err(unauthorized("the authorization is for another server"));
    }

    let pubkey = event.pubkey.to_hex();
    if !route.instance.state().snapshot.has_access(&pubkey) {
        return Err(Refused::new(StatusCode::FORBIDDEN, "unauthorized"));
    }

    let hashes = event.tags.values("x").filter_map(parse_name).collect();
    Ok((pubkey, hashes))
}

/// BUD-01: a blob's bytes, in whole or by range, as a download unless it is a
/// type a browser displays harmlessly.
pub async fn download(
    Extension(route): Extension<Route>,
    Path(name): Path<String>,
    request: Request,
) -> Answer {
    require_media(&route)?;
    let state = route.instance.state();
    let sha256 =
        parse_name(&name).ok_or_else(|| Refused::new(StatusCode::NOT_FOUND, "not a blob"))?;

    if state.relay.blossom_authenticated_read {
        authorize(&route, request.headers(), "get")?;
    }

    let core = route.instance.core();
    let blob = caravel_core::media::get_blob(core, &state.relay.id, &sha256)
        .await
        .map_err(|error| Refused::failed("the blob could not be read", error))?
        .ok_or_else(|| Refused::new(StatusCode::NOT_FOUND, "no such blob"))?;

    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| parse_range(value, blob.size as u64));
    let read = core
        .blobs
        .get(
            &caravel_relays::media::blob_key(&state.relay.id, &sha256),
            range.clone(),
        )
        .await
        .map_err(|error| Refused::failed("the blob could not be read", error))?
        .ok_or_else(|| Refused::new(StatusCode::NOT_FOUND, "no such blob"))?;

    let status = if range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let body = if request.method() == Method::HEAD {
        Body::empty()
    } else {
        Body::from_stream(read.stream)
    };
    let mut response = (status, body).into_response();
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&blob.content_type) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from(read.range.end - read.range.start),
    );
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if range.is_some()
        && let Ok(value) = HeaderValue::from_str(&format!(
            "bytes {}-{}/{}",
            read.range.start,
            read.range.end.saturating_sub(1),
            read.size
        ))
    {
        headers.insert(header::CONTENT_RANGE, value);
    }
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    harden(headers, &blob.content_type);

    Ok(response)
}

/// The bytes one `Range: bytes=` header asks for, when it asks for a single range
/// within the blob.
fn parse_range(header: &str, size: u64) -> Option<Range<u64>> {
    let spec = header.strip_prefix("bytes=")?;
    if spec.contains(',') || size == 0 {
        return None;
    }
    let (start, end) = spec.split_once('-')?;
    let range = match (start.trim(), end.trim()) {
        ("", suffix) => {
            let suffix: u64 = suffix.parse().ok()?;
            size.saturating_sub(suffix)..size
        }
        (start, "") => start.parse().ok()?..size,
        (start, end) => start.parse().ok()?..end.parse::<u64>().ok()?.saturating_add(1).min(size),
    };
    (range.start < range.end).then_some(range)
}

/// A blob's own content type can't run on the relay's origin.
fn harden(headers: &mut HeaderMap, content_type: &str) {
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );

    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    if !INLINE_TYPES.contains(&essence.as_str()) {
        headers.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment"),
        );
    }
}

/// BUD-06: whether an upload would be taken, before it is sent.
pub async fn check_upload(Extension(route): Extension<Route>, headers: HeaderMap) -> Answer {
    require_media(&route)?;
    authorize(&route, &headers, "upload")?;

    match headers
        .get("x-content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        Some(size) if size > MAX_BLOB_SIZE => Err(too_large()),
        Some(_) => Ok(StatusCode::OK.into_response()),
        None => Err(Refused::new(
            StatusCode::LENGTH_REQUIRED,
            "X-Content-Length is required",
        )),
    }
}

/// BUD-02: store an upload for its signer.
pub async fn upload(Extension(route): Extension<Route>, request: Request) -> Answer {
    require_media(&route)?;
    let (uploader, covered) = authorize(&route, request.headers(), "upload")?;

    if request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|size| size > MAX_BLOB_SIZE)
    {
        return Err(too_large());
    }

    let content_type = content_type(request.headers());
    let mut body = request.into_body().into_data_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next().await {
        let chunk = chunk
            .map_err(|_| Refused::new(StatusCode::BAD_REQUEST, "the file did not arrive whole"))?;
        bytes.extend_from_slice(&chunk);
        if bytes.len() as u64 > MAX_BLOB_SIZE {
            return Err(too_large());
        }
    }

    let blob = keep(
        &route,
        &uploader,
        Bytes::from(bytes),
        content_type,
        &covered,
    )
    .await?;
    Ok(axum::Json(descriptor(&route, &blob)).into_response())
}

/// BUD-04: copy a file from another server, given its url and an authorization
/// naming its hash.
pub async fn mirror(Extension(route): Extension<Route>, headers: HeaderMap, body: Bytes) -> Answer {
    require_media(&route)?;
    let (uploader, covered) = authorize(&route, &headers, "upload")?;
    if covered.is_empty() {
        return Err(Refused::new(
            StatusCode::BAD_REQUEST,
            "a mirror's authorization has to name the file's hash",
        ));
    }

    let url = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|body| body["url"].as_str().map(str::to_string))
        .ok_or_else(|| Refused::new(StatusCode::BAD_REQUEST, "expected {\"url\": ...}"))?;

    let fetched = route
        .instance
        .core()
        .outbound
        .fetch(&url, MAX_BLOB_SIZE)
        .await
        .map_err(|error| {
            Refused::new(
                StatusCode::BAD_GATEWAY,
                format!("could not fetch it: {error}"),
            )
        })?;
    let content_type = fetched
        .content_type
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let blob = keep(&route, &uploader, fetched.bytes, content_type, &covered).await?;
    Ok(axum::Json(descriptor(&route, &blob)).into_response())
}

/// Store bytes as a blob owned by `pubkey`, if they are one of the hashes the
/// authorization covers, where it names any.
async fn keep(
    route: &Route,
    pubkey: &str,
    bytes: Bytes,
    content_type: String,
    covered: &[String],
) -> Result<Blob, Refused> {
    let sha256 = sha256_hex(&bytes);
    if !covered.is_empty() && !covered.contains(&sha256) {
        return Err(Refused::new(
            StatusCode::FORBIDDEN,
            "the authorization does not cover this file",
        ));
    }

    let relay_id = route.instance.relay_id();
    media::keep_upload(
        route.instance.core(),
        &relay_id,
        &sha256,
        &content_type,
        pubkey,
        bytes,
    )
    .await
    .map_err(|error| Refused::failed("the file could not be kept", error))
}

/// BUD-02: the blobs a pubkey uploaded, for the relay's members.
pub async fn list(
    Extension(route): Extension<Route>,
    Path(pubkey): Path<String>,
    headers: HeaderMap,
) -> Answer {
    require_media(&route)?;
    authorize(&route, &headers, "list")?;

    if !is_hex64(&pubkey) {
        return Err(Refused::new(StatusCode::BAD_REQUEST, "not a hex pubkey"));
    }

    let blobs = caravel_core::media::list_blobs_for_pubkey(
        route.instance.core(),
        &route.instance.relay_id(),
        &pubkey,
    )
    .await
    .map_err(|error| Refused::failed("the list could not be read", error))?;
    let descriptors: Vec<Value> = blobs.iter().map(|blob| descriptor(&route, blob)).collect();
    Ok(axum::Json(descriptors).into_response())
}

/// BUD-02: give up one's ownership of a blob, which goes with its last owner.
pub async fn delete(
    Extension(route): Extension<Route>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Answer {
    require_media(&route)?;
    let sha256 =
        parse_name(&name).ok_or_else(|| Refused::new(StatusCode::NOT_FOUND, "not a blob"))?;
    let (pubkey, covered) = authorize(&route, &headers, "delete")?;
    if !covered.contains(&sha256) {
        return Err(Refused::new(
            StatusCode::FORBIDDEN,
            "the authorization does not name this file",
        ));
    }

    let relay_id = route.instance.relay_id();
    let deleted = media::delete_upload(route.instance.core(), &relay_id, &sha256, &pubkey)
        .await
        .map_err(|error| Refused::failed("the blob could not be deleted", error))?;

    match deleted {
        Deleted::NotOwned => Err(Refused::new(StatusCode::NOT_FOUND, "you have no such blob")),
        Deleted::Shared | Deleted::Last => Ok(StatusCode::OK.into_response()),
    }
}

fn too_large() -> Refused {
    Refused::new(
        StatusCode::PAYLOAD_TOO_LARGE,
        format!("files here are at most {MAX_BLOB_SIZE} bytes"),
    )
}

fn content_type(headers: &HeaderMap) -> String {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_cover_one_span_within_the_blob() {
        assert_eq!(parse_range("bytes=0-9", 100), Some(0..10));
        assert_eq!(parse_range("bytes=90-", 100), Some(90..100));
        assert_eq!(parse_range("bytes=-10", 100), Some(90..100));
        assert_eq!(parse_range("bytes=95-200", 100), Some(95..100));
        assert_eq!(parse_range("bytes=0-1,5-6", 100), None);
        assert_eq!(parse_range("bytes=100-", 100), None);
    }

    #[test]
    fn names_are_hashes_with_an_optional_extension() {
        let hash = "a".repeat(64);
        assert_eq!(parse_name(&format!("{hash}.png")), Some(hash.clone()));
        assert_eq!(parse_name(&hash.to_uppercase()), Some(hash));
        assert_eq!(parse_name("favicon.ico"), None);
    }
}
