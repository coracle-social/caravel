//! The tenant dashboard, the admin dashboard and the member portal: single-page
//! apps built from `web/` and embedded in the binary. Each is served as an HTML
//! shell carrying its runtime configuration, so one build runs anywhere.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderValue, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use caravel_core::relays::Relay;
use serde_json::{Value, json};

#[derive(rust_embed::Embed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Dist;

/// One of the single-page apps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    Tenant,
    Admin,
    Portal,
}

impl App {
    fn shell(self) -> &'static str {
        match self {
            App::Tenant => "tenant.html",
            App::Admin => "admin.html",
            App::Portal => "portal.html",
        }
    }
}

#[derive(Clone)]
enum Files {
    Embedded,
    Given(Arc<HashMap<String, Vec<u8>>>),
}

/// The built apps, and the configuration every shell carries.
#[derive(Clone)]
pub struct Web {
    files: Files,
    config: Value,
}

impl Web {
    pub fn new(config: Value) -> Self {
        Self {
            files: Files::Embedded,
            config,
        }
    }

    /// Apps built from `files`, keyed by path, instead of the embedded build.
    pub fn with_files(config: Value, files: HashMap<String, Vec<u8>>) -> Self {
        Self {
            files: Files::Given(Arc::new(files)),
            config,
        }
    }

    fn read(&self, path: &str) -> Option<(Cow<'static, [u8]>, String)> {
        match &self.files {
            Files::Embedded => {
                Dist::get(path).map(|file| (file.data, file.metadata.mimetype().to_string()))
            }
            Files::Given(files) => files.get(path).map(|bytes| {
                let content_type = mime_guess::from_path(path).first_or_octet_stream();
                (Cow::Owned(bytes.clone()), content_type.to_string())
            }),
        }
    }

    /// A built file at a request path, such as a bundle under `/assets`. Hashed
    /// bundles are cached for good.
    pub fn file(&self, path: &str) -> Option<Response> {
        let name = path.trim_start_matches('/');
        if name.is_empty() || name.ends_with(".html") {
            return None;
        }
        let (bytes, content_type) = self.read(name)?;
        let cache = if name.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "public, max-age=3600"
        };

        Some(
            (
                [
                    (header::CONTENT_TYPE, content_type),
                    (header::CACHE_CONTROL, cache.to_string()),
                ],
                Body::from(bytes.into_owned()),
            )
                .into_response(),
        )
    }

    /// An app's HTML shell, carrying the shared configuration with `context`
    /// merged in.
    pub fn shell(&self, app: App, context: Value) -> Response {
        let Some((html, _)) = self.read(app.shell()) else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "The web apps aren't built.",
            )
                .into_response();
        };

        let mut config = self.config.clone();
        if let (Some(config), Value::Object(context)) = (config.as_object_mut(), context) {
            config.extend(context);
        }
        // A `<` in a value can't end the script element it sits in.
        let config = config.to_string().replace('<', "\\u003c");
        let script = format!(r#"<script id="config" type="application/json">{config}</script>"#);
        let html =
            String::from_utf8_lossy(&html).replacen("</head>", &format!("{script}</head>"), 1);

        (
            [
                (
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                ),
                (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
            ],
            html,
        )
            .into_response()
    }
}

/// What the portal knows of the relay it is served for.
pub fn portal_context(relay: &Relay) -> Value {
    json!({
        "relay": {
            "id": relay.id,
            "host": relay.host(),
            "name": relay.display_name(),
            "icon": relay.info_icon,
            "description": relay.info_description,
        },
    })
}

/// Whether a request is a browser navigating to a page, rather than a protocol
/// request or a blob opened in a tab.
pub fn is_page_request<B>(request: &Request<B>) -> bool {
    let headers = request.headers();
    let path = request.uri().path();
    let accepts_html = headers
        .get(header::ACCEPT)
        .and_then(|accept| accept.to_str().ok())
        .is_some_and(|accept| accept.contains("text/html"));

    matches!(*request.method(), Method::GET | Method::HEAD)
        && accepts_html
        && !headers.contains_key(header::UPGRADE)
        && !path.starts_with("/.well-known/")
        && !is_blob_path(path)
}

/// `/<sha256>`, with or without an extension, as Blossom serves blobs.
fn is_blob_path(path: &str) -> bool {
    let name = path.trim_start_matches('/');
    let hash = name.split('.').next().unwrap_or_default();
    !name.contains('/') && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit())
}
