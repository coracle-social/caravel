//! The virtual relays: every live relay served from this process, each on its own
//! hosts, from the shared event store. A relay is loaded when the process starts
//! and reloaded whenever the core says it changed; nothing on the path from a
//! request's host to its relay touches the database.

pub mod auth;
pub mod blossom;
pub mod connections;
pub mod instance;
pub mod limits;
pub mod livekit;
pub mod message;
pub mod nip11;
pub mod nip86;
pub mod push;
pub mod session;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, OnceLock, RwLock};

use axum::Router;
use axum::extract::{ConnectInfo, Extension, FromRequestParts, Request, State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post, put};
use caravel_core::bus::Change;
use caravel_core::relays::{self, Relay, RelayStatus};
use caravel_core::{Core, Result};
use coracle_lib::events::Event;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

use crate::instance::{InstanceState, RelayInstance};
use crate::limits::Limiter;
use crate::push::Pusher;
use crate::session::Transport;

/// How much any one connection may ask of a relay.
#[derive(Clone, Copy, Debug)]
pub struct RelayLimits {
    pub max_subscriptions: usize,
    pub max_identities: usize,
    pub max_reconciliations: usize,
    /// The most stored events one filter returns.
    pub max_limit: usize,
    /// The most events one reconciliation covers.
    pub max_sync: usize,
}

impl Default for RelayLimits {
    fn default() -> Self {
        Self {
            max_subscriptions: 300,
            max_identities: 16,
            max_reconciliations: 8,
            max_limit: 1000,
            max_sync: 20_000,
        }
    }
}

/// How the relays are served.
#[derive(Clone, Debug, Default)]
pub struct RelaysConfig {
    pub limits: RelayLimits,
    pub transport: Transport,
    /// The header a trusted proxy reports the client's address in, such as
    /// `X-Forwarded-For`. Without one, the peer address is the client's.
    pub forwarded_header: Option<String>,
    /// New connections allowed per minute from one address; zero is unlimited.
    pub connections_per_minute: u32,
}

/// A request's host resolved to the relay it belongs to.
#[derive(Clone)]
pub struct Route {
    pub instance: Arc<RelayInstance>,
    /// The host the request came in on.
    pub host: String,
    /// The host the relay is known by, which pages on its other hosts redirect to.
    pub primary_host: String,
}

/// Every relay this process serves, by id and by host.
#[derive(Clone)]
pub struct Relays {
    inner: Arc<Inner>,
}

struct Inner {
    core: Core,
    config: RelaysConfig,
    connecting: Limiter,
    pusher: Pusher,
    shutdown: CancellationToken,
    instances: RwLock<HashMap<String, Arc<RelayInstance>>>,
    hosts: RwLock<HashMap<String, (String, String)>>,
    router: OnceLock<Router>,
}

impl Relays {
    /// Load every live relay. `shutdown` closes every connection when cancelled.
    pub async fn load(
        core: Core,
        config: RelaysConfig,
        shutdown: CancellationToken,
    ) -> Result<Self> {
        let relays = Self {
            inner: Arc::new(Inner {
                connecting: Limiter::new(config.connections_per_minute),
                pusher: Pusher::start(core.clone(), shutdown.clone()),
                core,
                config,
                shutdown,
                instances: RwLock::default(),
                hosts: RwLock::default(),
                router: OnceLock::new(),
            }),
        };
        relays.reload_all().await?;
        Ok(relays)
    }

    /// The relay a host belongs to, if any.
    pub fn resolve(&self, host: &str) -> Option<Route> {
        let (relay_id, primary_host) = self.inner.hosts.read().ok()?.get(host).cloned()?;
        let instance = self.inner.instances.read().ok()?.get(&relay_id).cloned()?;

        Some(Route {
            instance,
            host: host.to_string(),
            primary_host,
        })
    }

    /// Answer a request on a relay's host: a websocket, its information document,
    /// or a page.
    pub async fn serve(&self, route: Route, mut request: Request) -> Response {
        let router = self.inner.router.get_or_init(|| {
            Router::new()
                .route("/", get(root).post(management).options(preflight))
                .route(
                    "/upload",
                    put(blossom::upload)
                        .head(blossom::check_upload)
                        .options(blossom::preflight),
                )
                .route("/mirror", put(blossom::mirror).options(blossom::preflight))
                .route(
                    "/list/{pubkey}",
                    get(blossom::list).options(blossom::preflight),
                )
                .route(
                    "/{name}",
                    get(blossom::download)
                        .delete(blossom::delete)
                        .options(blossom::preflight),
                )
                .route(
                    "/.well-known/nip29/livekit",
                    get(livekit::support).options(livekit::preflight),
                )
                .route(
                    "/.well-known/nip29/livekit/{room_id}",
                    get(livekit::token).options(livekit::preflight),
                )
                .route("/.well-known/nip29/livekit/webhook", post(livekit::webhook))
                .fallback(|| async { StatusCode::NOT_FOUND })
                .with_state(self.clone())
        });

        request.extensions_mut().insert(route);
        match router.clone().oneshot(request).await {
            Ok(response) => response,
            Err(infallible) => match infallible {},
        }
    }

    /// Reload relays as the core reports changes to them, and all of them after
    /// missing some, until `shutdown`.
    pub async fn watch(&self, shutdown: CancellationToken) {
        let mut changes = self.inner.core.bus.subscribe();

        loop {
            let change = tokio::select! {
                _ = shutdown.cancelled() => break,
                change = changes.recv() => change,
            };

            let result = match change {
                Ok(Change::RelayChanged { relay_id }) => self.reload(&relay_id).await,
                Ok(Change::AccessChanged {
                    relay_id,
                    published,
                }) => {
                    self.deliver_access_change(&relay_id, &published).await;
                    Ok(())
                }
                Ok(Change::Notified {
                    relay_id,
                    pubkey,
                    message,
                }) => {
                    if let Some(instance) = self.instance(&relay_id) {
                        instance.deliver_notification(&pubkey, &message);
                    }
                    Ok(())
                }
                Err(RecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "relay reloads lagged");
                    self.reload_all().await
                }
                Err(RecvError::Closed) => break,
            };

            if let Err(error) = result {
                tracing::error!(error = ?error, "failed to reload relays");
            }
        }
    }

    /// Bring one relay up to date with its row: serve it if it is live, and stop
    /// serving it otherwise.
    pub async fn reload(&self, relay_id: &str) -> Result<()> {
        match relays::get_relay(self.inner.core.db.pool(), relay_id).await? {
            Some(relay) if is_served(&relay) => self.install(relay).await?,
            _ => self.remove(relay_id),
        }
        Ok(())
    }

    /// The relay served under an id.
    pub fn instance(&self, relay_id: &str) -> Option<Arc<RelayInstance>> {
        self.inner
            .instances
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .get(relay_id)
            .cloned()
    }

    /// Reload a relay's access lists and rooms after a change made outside it, then
    /// deliver what the relay published about the change.
    async fn deliver_access_change(&self, relay_id: &str, published: &[Event]) {
        if let Some(instance) = self.instance(relay_id) {
            instance.refresh().await;
            for event in published {
                instance.fanout(event);
            }
        }
    }

    async fn reload_all(&self) -> Result<()> {
        let live: Vec<Relay> = relays::list_relays(self.inner.core.db.pool())
            .await?
            .into_iter()
            .filter(is_served)
            .collect();

        let stale: Vec<String> = {
            let instances = self
                .inner
                .instances
                .read()
                .unwrap_or_else(|error| error.into_inner());
            instances
                .keys()
                .filter(|id| !live.iter().any(|relay| &relay.id == *id))
                .cloned()
                .collect()
        };
        for relay_id in stale {
            self.remove(&relay_id);
        }
        for relay in live {
            if let Err(error) = self.install(relay.clone()).await {
                tracing::error!(relay = %relay.id, error = %error, "failed to load relay");
            }
        }
        Ok(())
    }

    /// Serve a relay: reload it if it is served already, and build it otherwise.
    async fn install(&self, relay: Relay) -> Result<()> {
        let existing = self
            .inner
            .instances
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .get(&relay.id)
            .cloned();

        match existing {
            Some(instance) => instance.reload(relay.clone()).await?,
            None => {
                let state = InstanceState::load(&self.inner.core, relay.clone()).await?;
                let instance = RelayInstance::new(
                    self.inner.core.clone(),
                    self.inner.config.limits,
                    state,
                    self.inner.pusher.clone(),
                    self.inner.shutdown.child_token(),
                );
                self.inner
                    .instances
                    .write()
                    .unwrap_or_else(|error| error.into_inner())
                    .entry(relay.id.clone())
                    .or_insert_with(|| Arc::new(instance));
            }
        }

        let primary = relay.host();
        let mut hosts = self
            .inner
            .hosts
            .write()
            .unwrap_or_else(|error| error.into_inner());
        hosts.retain(|_, (relay_id, _)| relay_id != &relay.id);
        for host in relay
            .redirect_from
            .iter()
            .chain([&relay.canonical_host(), &primary])
        {
            hosts.insert(host.clone(), (relay.id.clone(), primary.clone()));
        }

        Ok(())
    }

    fn remove(&self, relay_id: &str) {
        let removed = self
            .inner
            .instances
            .write()
            .unwrap_or_else(|error| error.into_inner())
            .remove(relay_id);
        self.inner
            .hosts
            .write()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|_, (id, _)| id != relay_id);

        if let Some(instance) = removed {
            instance.close();
        }
    }

    fn client_ip(&self, headers: &HeaderMap, addr: SocketAddr) -> IpAddr {
        self.inner
            .config
            .forwarded_header
            .as_deref()
            .and_then(|name| headers.get(name))
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.rsplit(',').next())
            .and_then(|ip| ip.trim().parse().ok())
            .unwrap_or(addr.ip())
    }
}

/// Whether a relay is served: active and not purged.
fn is_served(relay: &Relay) -> bool {
    relay.status == RelayStatus::Active && !relay.is_purged()
}

async fn root(
    State(relays): State<Relays>,
    Extension(route): Extension<Route>,
    request: Request,
) -> Response {
    let headers = request.headers();

    if wants_relay_info(headers) {
        let state = route.instance.state();
        return nip11::response(nip11::document(
            &state,
            &relays.inner.config.limits,
            &relays.inner.config.transport,
        ));
    }

    if !is_upgrade(headers) {
        if route.host != route.primary_host {
            return Redirect::permanent(&format!("https://{}/", route.primary_host))
                .into_response();
        }
        return (
            StatusCode::OK,
            "This is a nostr relay. Connect to it with a nostr client.\n",
        )
            .into_response();
    }

    let addr = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map_or(SocketAddr::from(([0, 0, 0, 0], 0)), |info| info.0);
    let ip = relays.client_ip(headers, addr);
    if !relays.inner.connecting.allow(ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "too many connections from this address",
        )
            .into_response();
    }

    let (mut parts, _) = request.into_parts();
    let upgrade = match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(upgrade) => upgrade,
        Err(rejection) => return rejection.into_response(),
    };

    let transport = relays.inner.config.transport;
    let addr = SocketAddr::new(ip, addr.port());
    let closed = route.instance.closed();
    upgrade
        .max_message_size(transport.max_message_size)
        .on_upgrade(move |socket| {
            session::run(socket, addr, route.host, route.instance, transport, closed)
        })
}

/// NIP-86: management calls arrive as `POST`s to the relay's url.
async fn management(
    Extension(route): Extension<Route>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    nip86::handle(&route, &headers, &body).await
}

/// Browsers preflight the information document, since they won't ask for
/// `application/nostr+json` without permission.
async fn preflight() -> Response {
    (
        StatusCode::NO_CONTENT,
        [
            (
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            ),
            (
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static("accept, authorization, content-type"),
            ),
            (
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("GET, POST, OPTIONS"),
            ),
        ],
    )
        .into_response()
}

fn is_upgrade(headers: &HeaderMap) -> bool {
    headers
        .get(header::UPGRADE)
        .and_then(|upgrade| upgrade.to_str().ok())
        .is_some_and(|upgrade| upgrade.eq_ignore_ascii_case("websocket"))
}

fn wants_relay_info(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|accept| accept.to_str().ok())
        .is_some_and(|accept| accept.contains("application/nostr+json"))
}
