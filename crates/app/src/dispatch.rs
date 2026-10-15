//! Routing a request by its host: a relay's hosts get that relay, and every other
//! host gets the API.

use axum::Router;
use axum::extract::{Request, State};
use axum::http::header;
use axum::response::Response;
use caravel_relay::Relays;
use tower::ServiceExt;

#[derive(Clone)]
struct Dispatch {
    api: Router,
    relays: Relays,
}

pub fn router(api: Router, relays: Relays) -> Router {
    Router::new()
        .fallback(dispatch)
        .with_state(Dispatch { api, relays })
}

async fn dispatch(State(dispatch): State<Dispatch>, request: Request) -> Response {
    if let Some(route) = dispatch.relays.resolve(&request_host(&request)) {
        return dispatch.relays.serve(route, request).await;
    }

    match dispatch.api.oneshot(request).await {
        Ok(response) => response,
        Err(infallible) => match infallible {},
    }
}

/// The host a request is for, lowercased and without a port: from the `Host`
/// header, or the request target's authority under HTTP/2.
pub fn request_host(request: &Request) -> String {
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
    let host = host.rsplit_once(':').map_or(host, |(name, port)| {
        if port.chars().all(|c| c.is_ascii_digit()) {
            name
        } else {
            host
        }
    });
    host.trim_end_matches('.').to_lowercase()
}
