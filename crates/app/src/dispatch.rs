//! Routing a request by its host. A relay's hosts get that relay's protocols and
//! its member portal; the API host gets the API; the platform host gets the
//! dashboards, and the API too when it shares the host under a path. Any other
//! host gets nothing, except the health check and the TLS ask, which answer on
//! every host.

use anyhow::{Result, bail};
use axum::Router;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Redirect, Response};
use caravel_relay::{Relays, Route};
use tower::ServiceExt;

use crate::web::{self, App, Web};

/// The hosts the API and the dashboards are served on.
#[derive(Clone, Debug)]
pub struct Hosts {
    pub api: String,
    /// The path the API is served under, empty for the root.
    pub api_prefix: String,
    /// The dashboards' host, if this process serves them.
    pub platform: Option<String>,
}

impl Hosts {
    /// The hosts in the API's and the dashboards' public urls. An API sharing the
    /// dashboards' host must sit under a path of its own.
    pub fn from_urls(api_url: &str, platform_url: Option<&str>) -> Result<Self> {
        let api_url = url::Url::parse(api_url)?;
        let platform = platform_url
            .map(url::Url::parse)
            .transpose()?
            .and_then(|url| url.host_str().map(str::to_lowercase));
        let hosts = Self {
            api: api_url.host_str().unwrap_or_default().to_lowercase(),
            api_prefix: api_url.path().trim_end_matches('/').to_string(),
            platform,
        };

        if hosts.platform.as_deref() == Some(hosts.api.as_str()) && hosts.api_prefix.is_empty() {
            bail!("API_URL needs a path, such as /api, to share PLATFORM_URL's host");
        }
        Ok(hosts)
    }
}

#[derive(Clone)]
struct Dispatch {
    api: Router,
    relays: Relays,
    web: Web,
    hosts: Hosts,
}

pub fn router(api: Router, relays: Relays, web: Web, hosts: Hosts) -> Router {
    Router::new().fallback(dispatch).with_state(Dispatch {
        api,
        relays,
        web,
        hosts,
    })
}

async fn dispatch(State(dispatch): State<Dispatch>, request: Request) -> Response {
    let host = request_host(&request);
    let path = request.uri().path().to_string();

    match path.as_str() {
        "/healthz" => return (StatusCode::OK, "ok").into_response(),
        "/domains/check" => return call(&dispatch.api, request).await,
        _ => {}
    }

    // The platform's own hosts come before relays', so no relay can take one over.
    let prefix = &dispatch.hosts.api_prefix;
    let on_platform = dispatch.hosts.platform.as_deref() == Some(host.as_str());
    if host == dispatch.hosts.api && (!on_platform || is_under(&path, prefix)) {
        return call(&dispatch.api, strip_prefix(request, prefix)).await;
    }

    if on_platform {
        return platform_host(&dispatch, &request);
    }

    if let Some(route) = dispatch.relays.resolve(&host) {
        return relay_host(&dispatch, route, request).await;
    }

    StatusCode::NOT_FOUND.into_response()
}

/// A relay's protocols, or its member portal for a browser. A page on any host
/// but the relay's primary one redirects there.
async fn relay_host(dispatch: &Dispatch, route: Route, request: Request) -> Response {
    let path = request.uri().path();
    if path.starts_with("/assets/") {
        return dispatch
            .web
            .file(path)
            .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response());
    }

    if web::is_page_request(&request) {
        let primary_host = route.primary_host();
        if route.host != primary_host {
            let target = request
                .uri()
                .path_and_query()
                .map_or("/", |path| path.as_str());
            return Redirect::permanent(&format!("https://{primary_host}{target}")).into_response();
        }
        let relay = route.instance.state().relay.profile();
        return dispatch.web.shell(App::Portal, Some(relay));
    }

    dispatch.relays.serve(route, request).await
}

/// The dashboards: the admin one under `/admin`, built files by path, and the
/// tenant one for every other page.
fn platform_host(dispatch: &Dispatch, request: &Request) -> Response {
    let path = request.uri().path();
    if let Some(file) = dispatch.web.file(path) {
        return file;
    }
    if path.starts_with("/assets/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let app = if is_under(path, "/admin") {
        App::Admin
    } else {
        App::Tenant
    };
    dispatch.web.shell(app, None)
}

async fn call(router: &Router, request: Request) -> Response {
    match router.clone().oneshot(request).await {
        Ok(response) => response,
        Err(infallible) => match infallible {},
    }
}

/// Whether `path` is `prefix` or below it. Everything is below the root.
fn is_under(path: &str, prefix: &str) -> bool {
    prefix.is_empty() || path == prefix || path.starts_with(&format!("{prefix}/"))
}

/// The request with `prefix` dropped from its path, if the path is under it.
fn strip_prefix(mut request: Request, prefix: &str) -> Request {
    if prefix.is_empty() || !is_under(request.uri().path(), prefix) {
        return request;
    }

    let path_and_query = request
        .uri()
        .path_and_query()
        .map_or("/", |path| path.as_str());
    let stripped = path_and_query
        .strip_prefix(prefix)
        .unwrap_or(path_and_query);
    let stripped = if stripped.starts_with('/') {
        stripped.to_string()
    } else {
        format!("/{stripped}")
    };
    if let Ok(uri) = stripped.parse::<Uri>() {
        *request.uri_mut() = uri;
    }
    request
}

/// The host a request is for, lowercased and without a port: from the `Host`
/// header, or the request target's authority under HTTP/2.
fn request_host(request: &Request) -> String {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .or_else(|| {
            request
                .uri()
                .authority()
                .map(|authority| authority.as_str())
        })
        .unwrap_or_default();
    caravel_util::urls::url_host(host)
}
