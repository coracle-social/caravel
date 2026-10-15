//! A relay server on a real socket for protocol tests, and a client that speaks
//! to it under a relay's host.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use axum::Router;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use caravel_core::Core;
use caravel_core::relays::Relay;
use caravel_core::testing;
use caravel_relay::{Relays, RelaysConfig};
use coracle_lib::events::Event;
use coracle_lib::keys::SecretKey;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tokio_util::sync::CancellationToken;

pub struct Server {
    pub core: Core,
    pub relays: Relays,
    pub addr: SocketAddr,
    pub shutdown: CancellationToken,
}

impl Server {
    pub async fn start() -> Server {
        let core = testing::core(&SecretKey::generate().public_key().to_hex()).await;
        let shutdown = CancellationToken::new();
        let relays = Relays::load(
            core.clone(),
            RelaysConfig::default(),
            shutdown.child_token(),
        )
        .await
        .expect("failed to load relays");

        tokio::spawn({
            let (relays, shutdown) = (relays.clone(), shutdown.clone());
            async move { relays.watch(shutdown).await }
        });

        let app = Router::new().fallback(dispatch).with_state(relays.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind");
        let addr = listener.local_addr().expect("no local address");
        let graceful = shutdown.clone();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move { graceful.cancelled().await })
            .await
            .expect("server failed");
        });

        Server {
            core,
            relays,
            addr,
            shutdown,
        }
    }

    /// A new relay, served from now on, with its tenant's key.
    pub async fn relay(&self) -> (Relay, SecretKey) {
        let (relay, owner) = testing::create_relay(&self.core).await;
        self.relays
            .reload(&relay.id)
            .await
            .expect("failed to load relay");
        (relay, owner)
    }

    /// Change a relay's settings and reload it.
    pub async fn update(&self, relay: &Relay, settings: Value) -> Relay {
        let settings = serde_json::from_value(settings).expect("invalid settings");
        let relay = caravel_core::hosting::update_relay(&self.core, relay.clone(), settings)
            .await
            .expect("failed to update relay");
        self.relays
            .reload(&relay.id)
            .await
            .expect("failed to reload relay");
        relay
    }

    pub async fn connect(&self, host: &str) -> Client {
        let mut request = format!("ws://{}/", self.addr)
            .into_client_request()
            .expect("invalid request");
        request
            .headers_mut()
            .insert("Host", host.parse().expect("invalid host"));
        let (ws, _) = tokio_tungstenite::connect_async(request)
            .await
            .expect("failed to connect");

        let mut client = Client {
            ws,
            host: host.to_string(),
            challenge: String::new(),
        };
        let first = client.recv().await.expect("no challenge");
        assert_eq!(
            first[0], "AUTH",
            "a connection is challenged first: {first}"
        );
        client.challenge = first[1].as_str().unwrap().to_string();
        client
    }

    pub fn now(&self) -> i64 {
        self.core.clock.now()
    }

    /// Make a NIP-86 call on a relay's host as `signer`, returning the response body.
    pub async fn manage(
        &self,
        host: &str,
        signer: &SecretKey,
        method: &str,
        params: Value,
    ) -> Value {
        let body = json!({ "method": method, "params": params }).to_string();
        let payload = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(body.as_bytes()));
        let auth = testing::sign_event(
            signer,
            27235,
            "",
            &[
                &["u", &format!("https://{host}/")],
                &["method", "POST"],
                &["payload", &payload],
            ],
            self.now(),
        );
        self.manage_with(host, &auth, body).await
    }

    pub async fn manage_with(&self, host: &str, auth: &Event, body: String) -> Value {
        use base64::Engine;

        let route = self.relays.resolve(host).expect("relay not served");
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/")
            .header("Host", host)
            .header("Content-Type", "application/nostr+json+rpc")
            .header(
                "Authorization",
                format!(
                    "Nostr {}",
                    base64::engine::general_purpose::STANDARD
                        .encode(serde_json::to_string(auth).unwrap())
                ),
            )
            .body(axum::body::Body::from(body))
            .unwrap();
        let response = self.relays.serve(route, request).await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }
}

async fn dispatch(State(relays): State<Relays>, request: Request) -> Response {
    let host = request
        .headers()
        .get("host")
        .and_then(|host| host.to_str().ok())
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .to_string();

    match relays.resolve(&host) {
        Some(route) => relays.serve(route, request).await,
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub struct Client {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    pub host: String,
    pub challenge: String,
}

impl Client {
    pub async fn send(&mut self, message: Value) {
        self.ws
            .send(Message::Text(message.to_string().into()))
            .await
            .expect("failed to send");
    }

    /// The next message, or an error if none comes within two seconds or the
    /// connection closes.
    pub async fn recv(&mut self) -> Result<Value> {
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(2), self.ws.next())
                .await
                .map_err(|_| anyhow!("timed out"))?
                .ok_or_else(|| anyhow!("connection ended"))??;
            match frame {
                Message::Text(text) => return Ok(serde_json::from_str(&text)?),
                Message::Close(frame) => bail!("closed: {frame:?}"),
                _ => continue,
            }
        }
    }

    /// The next message other than an `AUTH` challenge.
    pub async fn recv_skipping_auth(&mut self) -> Result<Value> {
        loop {
            let message = self.recv().await?;
            if message[0] != "AUTH" {
                return Ok(message);
            }
        }
    }

    /// Whether nothing arrives for a quarter second.
    pub async fn is_quiet(&mut self) -> bool {
        tokio::time::timeout(Duration::from_millis(250), self.ws.next())
            .await
            .is_err()
    }

    pub async fn auth(&mut self, secret: &SecretKey, now: i64) {
        let relay = format!("wss://{}/", self.host);
        let event = testing::sign_event(
            secret,
            22242,
            "",
            &[&["relay", &relay], &["challenge", &self.challenge.clone()]],
            now,
        );
        let (accepted, message) = self.publish_as("AUTH", &event).await;
        assert!(accepted, "auth refused: {message}");
    }

    pub async fn publish(&mut self, event: &Event) -> (bool, String) {
        self.publish_as("EVENT", event).await
    }

    async fn publish_as(&mut self, verb: &str, event: &Event) -> (bool, String) {
        self.send(json!([verb, event])).await;
        loop {
            let message = self.recv().await.expect("no OK");
            if message[0] == "OK" && message[1] == event.id.to_hex() {
                return (
                    message[2].as_bool().unwrap(),
                    message[3].as_str().unwrap().to_string(),
                );
            }
        }
    }

    /// Open a subscription and collect its stored events, returning them and the
    /// message that ended them: `EOSE`, or `CLOSED`.
    pub async fn req(&mut self, subscription_id: &str, filter: Value) -> (Vec<Value>, Value) {
        self.send(json!(["REQ", subscription_id, filter])).await;
        let mut events = Vec::new();
        loop {
            let message = self.recv_skipping_auth().await.expect("no EOSE");
            match message[0].as_str() {
                Some("EVENT") if message[1] == subscription_id => events.push(message[2].clone()),
                Some("EOSE" | "CLOSED") if message[1] == subscription_id => {
                    return (events, message);
                }
                _ => {}
            }
        }
    }
}
