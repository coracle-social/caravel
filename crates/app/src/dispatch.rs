//! Routing a request by its host and path. The API and the tenant and admin
//! dashboards each answer at the url they're configured with; a relay's hosts get
//! that relay's protocols, and its member portal under `/members`. Any other host
//! gets nothing, except the health check and the TLS ask, which answer on every
//! host.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Redirect, Response};
use caravel_relay::{Relays, Route};
use caravel_util::config::{Config, PORTAL_PATH};
use caravel_util::urls::url_host;
use tower::ServiceExt;

use crate::web::{App, Web};

/// A part of the platform that answers at a url of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Api,
    App(App),
}

/// A section, and the host and path it answers at.
#[derive(Clone, Debug)]
struct Mount {
    host: String,
    /// The path the section is under, without a trailing slash, so `""` at the root.
    path: String,
    section: Section,
}

impl Mount {
    fn new(url: &str, section: Section) -> Self {
        let path =
            url::Url::parse(url).map_or_else(|_| String::new(), |url| url.path().to_string());
        Self {
            host: url_host(url),
            path: path.trim_end_matches('/').to_string(),
            section,
        }
    }
}

#[derive(Clone)]
struct Dispatch {
    api: Router,
    relays: Relays,
    web: Web,
    mounts: Arc<Vec<Mount>>,
}

pub fn router(api: Router, relays: Relays, web: Web, config: Arc<Config>) -> Router {
    let mut mounts = vec![
        Mount::new(&config.api_url, Section::Api),
        Mount::new(&config.tenant_url, Section::App(App::Tenant)),
        Mount::new(&config.admin_url, Section::App(App::Admin)),
    ];
    if let Some(url) = &config.legacy_api_url {
        mounts.push(Mount::new(url, Section::Api));
    }

    Router::new().fallback(dispatch).with_state(Dispatch {
        api,
        relays,
        web,
        mounts: Arc::new(mounts),
    })
}

async fn dispatch(State(dispatch): State<Dispatch>, request: Request) -> Response {
    let host = request_host(&request);

    match request.uri().path() {
        "/healthz" => return (StatusCode::OK, "ok").into_response(),
        "/domains/check" => return call(&dispatch.api, request).await,
        _ => {}
    }

    // The platform's own hosts come before relays', so no relay can take one over.
    if dispatch.mounts.iter().any(|mount| mount.host == host) {
        return platform_host(&dispatch, &host, request).await;
    }

    if let Some(route) = dispatch.relays.resolve(&host) {
        return relay_host(&dispatch, route, request).await;
    }

    StatusCode::NOT_FOUND.into_response()
}

/// Whichever section on this host the path is deepest under. The apps' built
/// files sit at the root of every platform host, since their pages ask for them
/// there.
async fn platform_host(dispatch: &Dispatch, host: &str, request: Request) -> Response {
    let path = request.uri().path();
    let mount = dispatch
        .mounts
        .iter()
        .filter(|mount| mount.host == host && is_under(path, &mount.path))
        .max_by_key(|mount| mount.path.len())
        .cloned();

    if let Some(file) = dispatch.web.file(path) {
        return file;
    }

    match mount {
        Some(Mount {
            section: Section::Api,
            path: prefix,
            ..
        }) => call(&dispatch.api, strip_prefix(request, &prefix)).await,
        _ if path.starts_with("/assets/") => StatusCode::NOT_FOUND.into_response(),
        _ if !matches!(*request.method(), Method::GET | Method::HEAD) => {
            StatusCode::METHOD_NOT_ALLOWED.into_response()
        }
        Some(Mount {
            section: Section::App(app),
            ..
        }) => dispatch.web.shell(app, None),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// A relay's protocols, and its member portal with the files its pages load. A
/// portal page on a host that isn't the relay's primary one redirects there.
async fn relay_host(dispatch: &Dispatch, route: Route, request: Request) -> Response {
    let path = request.uri().path();
    if path.starts_with("/assets/") {
        return dispatch
            .web
            .file(path)
            .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response());
    }

    if is_under(path, PORTAL_PATH) && matches!(*request.method(), Method::GET | Method::HEAD) {
        let primary_host = route.primary_host();
        if route.host != primary_host {
            let target = request
                .uri()
                .path_and_query()
                .map_or(path, |path| path.as_str());
            return Redirect::permanent(&format!("https://{primary_host}{target}")).into_response();
        }
        let relay = route.instance.state().relay.profile();
        return dispatch.web.shell(App::Portal, Some(relay));
    }

    dispatch.relays.serve(route, request).await
}

async fn call(router: &Router, request: Request) -> Response {
    match router.clone().oneshot(request).await {
        Ok(response) => response,
        Err(infallible) => match infallible {},
    }
}

/// Whether `path` is `prefix` or below it.
fn is_under(path: &str, prefix: &str) -> bool {
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

/// The request with `prefix` dropped from its path, if the path is under it.
fn strip_prefix(mut request: Request, prefix: &str) -> Request {
    if !is_under(request.uri().path(), prefix) {
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
    url_host(host)
}
