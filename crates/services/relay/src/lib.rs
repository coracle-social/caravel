//! The virtual relays: every live relay served from this process, each on its own
//! hosts, from the shared event store. A relay is loaded when the process starts
//! and reloaded whenever the core says it changed; nothing on the path from a
//! request's host to its relay touches the database.

mod auth;
pub mod blossom;
mod connections;
mod instance;
mod limits;
mod livekit;
mod message;
mod nip11;
mod nip86;
mod push;
mod session;

pub use session::Transport;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use axum::Router;
use axum::extract::{ConnectInfo, Extension, FromRequestParts, Request, State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderName, Method, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post, put};
use caravel_billing::relays::Relay;
use caravel_core::Core;
use caravel_util::bus::Change;
use caravel_util::error::Result;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use tower_http::cors::{Any, CorsLayer};

use crate::instance::{InstanceState, RelayInstance};
use crate::limits::Limiter;
use crate::push::Pusher;

/// How much any one connection may ask of a relay.
#[derive(Clone, Copy, Debug)]
pub struct RelayLimits {
    pub max_subscriptions: usize,
    pub max_identities: usize,
    pub max_reconciliations: usize,
    /// The most filters one `REQ` or `COUNT` may hold.
    pub max_filters: usize,
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
            max_filters: 200,
            max_limit: 1000,
            max_sync: 20_000,
        }
    }
}

/// New connections allowed per minute from one address, allowing a minute's worth
/// at once.
const CONNECTIONS_PER_MINUTE: u32 = 60;

/// How the relays are served.
#[derive(Clone, Debug, Default)]
pub struct RelaysConfig {
    pub limits: RelayLimits,
    pub transport: Transport,
    /// The header a trusted proxy reports the client's address in, such as
    /// `X-Forwarded-For`. Without one, the peer address is the client's.
    pub forwarded_header: Option<String>,
}

/// A request's host resolved to the relay it belongs to.
#[derive(Clone)]
pub struct Route {
    pub instance: Arc<RelayInstance>,
    /// The host the request came in on.
    pub host: String,
}

impl Route {
    /// The host the relay is known by, which pages on its other hosts redirect to.
    pub fn primary_host(&self) -> String {
        self.instance.state().relay.host()
    }
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
    /// The relay each host belongs to, by id.
    hosts: RwLock<HashMap<String, String>>,
    router: OnceLock<Router>,
    /// The bus as of before the first load, until `watch` takes it.
    changes: Mutex<Option<broadcast::Receiver<Change>>>,
}

impl Relays {
    /// Load every live relay, holding the changes published meanwhile for
    /// [`Relays::watch`]. `shutdown` closes every connection when cancelled.
    pub async fn load(
        core: Core,
        config: RelaysConfig,
        shutdown: CancellationToken,
    ) -> Result<Self> {
        let relays = Self {
            inner: Arc::new(Inner {
                connecting: Limiter::new(CONNECTIONS_PER_MINUTE),
                pusher: Pusher::start(core.clone(), shutdown.clone()),
                changes: Mutex::new(Some(core.bus.subscribe())),
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
        let relay_id = self
            .inner
            .hosts
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .get(host)
            .cloned()?;
        let instance = self.instance(&relay_id)?;

        Some(Route {
            instance,
            host: host.to_string(),
        })
    }

    /// Answer a protocol request on a relay's host: a websocket, its information
    /// document, NIP-86, Blossom or LiveKit.
    pub async fn serve(&self, route: Route, mut request: Request) -> Response {
        let router = self.inner.router.get_or_init(|| {
            Router::new()
                .route("/", get(root).post(nip86::handle))
                .route("/upload", put(blossom::upload).head(blossom::check_upload))
                .route("/mirror", put(blossom::mirror))
                .route("/list/{pubkey}", get(blossom::list))
                .route("/{name}", get(blossom::download).delete(blossom::delete))
                .route("/.well-known/nip29/livekit", get(livekit::support))
                .route("/.well-known/nip29/livekit/{room_id}", get(livekit::token))
                .route("/.well-known/nip29/livekit/webhook", post(livekit::webhook))
                .fallback(|| async { StatusCode::NOT_FOUND })
                .layer(cors())
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
        let held = self
            .inner
            .changes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let mut changes = held.unwrap_or_else(|| self.inner.core.bus.subscribe());

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
                    if let Some(instance) = self.instance(&relay_id) {
                        instance.apply_access_change(&published).await;
                    }
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
        match caravel_core::hosting::get_relay(&self.inner.core, relay_id).await? {
            Some(relay) if relay.is_served() => self.install(relay).await?,
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

    async fn reload_all(&self) -> Result<()> {
        let live: Vec<Relay> = caravel_core::hosting::list_relays(&self.inner.core)
            .await?
            .into_iter()
            .filter(Relay::is_served)
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

        let mut hosts = self
            .inner
            .hosts
            .write()
            .unwrap_or_else(|error| error.into_inner());
        hosts.retain(|_, relay_id| relay_id != &relay.id);
        for host in relay.hosts() {
            hosts.insert(host, relay.id.clone());
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
            .retain(|_, id| id != relay_id);

        self.inner.pusher.retain(relay_id, &[]);
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

async fn root(
    State(relays): State<Relays>,
    Extension(route): Extension<Route>,
    request: Request,
) -> Response {
    let headers = request.headers();

    if wants_relay_info(headers) {
        let state = route.instance.state();
        let limits = caravel_relays::profiles::Limits {
            max_limit: relays.inner.config.limits.max_limit,
            max_subscriptions: relays.inner.config.limits.max_subscriptions,
            max_message_size: relays.inner.config.transport.max_message_size,
        };
        return nip11::response(nip11::document(&state.relay.info(), &limits));
    }

    if !is_upgrade(headers) {
        let primary_host = route.primary_host();
        if route.host != primary_host {
            return Redirect::permanent(&format!("https://{primary_host}/")).into_response();
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

/// CORS for everything a relay serves over HTTP: any origin may call it, with
/// the headers NIP-98, Blossom and NIP-11 use, and read why media was refused.
fn cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::HEAD,
            Method::POST,
            Method::PUT,
            Method::DELETE,
        ])
        .allow_headers([
            header::ACCEPT,
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            HeaderName::from_static("x-content-length"),
            HeaderName::from_static("x-content-type"),
            HeaderName::from_static("x-sha-256"),
        ])
        .expose_headers([HeaderName::from_static("x-reason")])
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
