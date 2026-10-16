//! A client for other people's Nostr relays: publishing events to them, reading
//! events from them, and asking a question an event answers, as NIP-47 wallets
//! are asked. Each call opens its own connections and closes them when it is
//! done, and a connection answers a relay's NIP-42 challenge with its key.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use coracle_lib::events::{Event, EventContent};
use coracle_lib::filters::Filter;
use coracle_lib::keys::SecretKey;
use coracle_lib::tags::Tags;
use futures_util::future::{join_all, select_ok};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

/// The kind a client answers a relay's challenge with.
const CLIENT_AUTH: u16 = 22242;

/// A key to talk to relays as.
#[derive(Clone)]
pub struct Client {
    secret: SecretKey,
}

impl Client {
    pub fn new(secret: SecretKey) -> Self {
        Self { secret }
    }

    /// Publish an event to every relay, succeeding if any of them takes it.
    pub async fn publish(
        &self,
        relays: &[String],
        event: &Event,
        now: i64,
        timeout: Duration,
    ) -> Result<()> {
        let results = join_all(
            relays
                .iter()
                .map(|relay| within(timeout, self.publish_to(relay, event, now))),
        )
        .await;

        let mut errors = Vec::new();
        for result in results {
            match result {
                Ok(()) => return Ok(()),
                Err(error) => errors.push(error.to_string()),
            }
        }
        bail!("no relay took the event: {}", errors.join("; "))
    }

    /// The events relays hold matching a filter, gathered until each says it has
    /// sent them all, or the timeout passes.
    pub async fn fetch(
        &self,
        relays: &[String],
        filter: &Filter,
        now: i64,
        timeout: Duration,
    ) -> Result<Vec<Event>> {
        let results = join_all(
            relays
                .iter()
                .map(|relay| within(timeout, self.fetch_from(relay, filter, now))),
        )
        .await;

        let mut events = BTreeMap::new();
        let mut errors = Vec::new();
        for result in results {
            match result {
                Ok(fetched) => {
                    events.extend(fetched.into_iter().map(|event| (event.id.to_hex(), event)))
                }
                Err(error) => errors.push(error.to_string()),
            }
        }
        if !relays.is_empty() && errors.len() == relays.len() {
            bail!("no relay answered: {}", errors.join("; "));
        }
        Ok(events.into_values().collect())
    }

    /// Publish a request and wait for the first event any relay sends matching
    /// `reply`.
    pub async fn request(
        &self,
        relays: &[String],
        event: &Event,
        reply: &Filter,
        now: i64,
        timeout: Duration,
    ) -> Result<Event> {
        if relays.is_empty() {
            bail!("no relay to send the request to");
        }
        let attempts = relays
            .iter()
            .map(|relay| Box::pin(within(timeout, self.request_from(relay, event, reply, now))));
        Ok(select_ok(attempts).await?.0)
    }

    async fn publish_to(&self, relay: &str, event: &Event, now: i64) -> Result<()> {
        let mut connection = Connection::open(relay, &self.secret, now).await?;
        let publish = json!(["EVENT", event]);
        connection.send(&publish).await?;

        let mut retry = Retry::default();
        loop {
            match connection.receive().await? {
                Received::Authenticated => {
                    if retry.on_authenticated() {
                        connection.send(&publish).await?;
                    }
                }
                Received::Message(message) => {
                    if message[0] != "OK" || message[1] != event.id.to_hex() {
                        continue;
                    }
                    if message[2] == true {
                        connection.close().await;
                        return Ok(());
                    }
                    let reason = message[3].as_str().unwrap_or_default();
                    if retry.on_refused(reason, connection.authenticated) {
                        connection.send(&publish).await?;
                    } else if !retry.waiting {
                        bail!("{relay} refused the event: {reason}");
                    }
                }
            }
        }
    }

    async fn fetch_from(&self, relay: &str, filter: &Filter, now: i64) -> Result<Vec<Event>> {
        let mut connection = Connection::open(relay, &self.secret, now).await?;
        let subscribe = json!(["REQ", "fetch", filter]);
        connection.send(&subscribe).await?;

        let mut events = Vec::new();
        let mut retry = Retry::default();
        loop {
            match connection.receive().await? {
                Received::Authenticated => {
                    if retry.on_authenticated() {
                        connection.send(&subscribe).await?;
                    }
                }
                Received::Message(message) if message[1] != "fetch" => {}
                Received::Message(message) => match message[0].as_str() {
                    Some("EVENT") => {
                        if let Some(event) = verified(&message[2])
                            && filter.matches(&event)
                        {
                            events.push(event);
                        }
                    }
                    Some("EOSE") => break,
                    Some("CLOSED") => {
                        let reason = message[2].as_str().unwrap_or_default();
                        if retry.on_refused(reason, connection.authenticated) {
                            connection.send(&subscribe).await?;
                        } else if !retry.waiting {
                            bail!("{relay} closed the subscription: {reason}");
                        }
                    }
                    _ => {}
                },
            }
        }

        connection.send(&json!(["CLOSE", "fetch"])).await?;
        connection.close().await;
        Ok(events)
    }

    async fn request_from(
        &self,
        relay: &str,
        event: &Event,
        reply: &Filter,
        now: i64,
    ) -> Result<Event> {
        let mut connection = Connection::open(relay, &self.secret, now).await?;
        let subscribe = json!(["REQ", "reply", reply]);
        let publish = json!(["EVENT", event]);
        connection.send(&subscribe).await?;
        connection.send(&publish).await?;

        let mut retry = Retry::default();
        loop {
            match connection.receive().await? {
                Received::Authenticated => {
                    if retry.on_authenticated() {
                        connection.send(&subscribe).await?;
                        connection.send(&publish).await?;
                    }
                }
                Received::Message(message) => match message[0].as_str() {
                    Some("EVENT") if message[1] == "reply" => {
                        if let Some(answer) = verified(&message[2])
                            && reply.matches(&answer)
                        {
                            connection.close().await;
                            return Ok(answer);
                        }
                    }
                    Some("OK") if message[1] == event.id.to_hex() && message[2] != true => {
                        let reason = message[3].as_str().unwrap_or_default();
                        if retry.on_refused(reason, connection.authenticated) {
                            connection.send(&subscribe).await?;
                            connection.send(&publish).await?;
                        } else if !retry.waiting {
                            bail!("{relay} refused the request: {reason}");
                        }
                    }
                    Some("CLOSED") if message[1] == "reply" => {
                        let reason = message[2].as_str().unwrap_or_default();
                        if !reason.starts_with("auth-required") {
                            bail!("{relay} closed the subscription: {reason}");
                        }
                    }
                    _ => {}
                },
            }
        }
    }
}

/// Run `work` for at most `timeout`.
async fn within<T>(timeout: Duration, work: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(timeout, work)
        .await
        .map_err(|_| anyhow!("the relay didn't answer in time"))?
}

/// An event a relay sent, if it is one and its signature holds.
fn verified(value: &Value) -> Option<Event> {
    let event: Event = serde_json::from_value(value.clone()).ok()?;
    event.verify().ok().map(|()| event)
}

/// Whether to send something again after a relay refused it until the
/// connection authenticates, which happens at most once.
#[derive(Default)]
struct Retry {
    retried: bool,
    /// Refused for want of authentication before the relay challenged us.
    waiting: bool,
}

impl Retry {
    /// After a refusal: whether to send again now.
    fn on_refused(&mut self, reason: &str, authenticated: bool) -> bool {
        if self.retried || !reason.starts_with("auth-required") {
            return false;
        }
        self.retried = true;
        self.waiting = !authenticated;
        authenticated
    }

    /// After answering a challenge: whether a refusal is waiting to be sent again.
    fn on_authenticated(&mut self) -> bool {
        std::mem::take(&mut self.waiting)
    }
}

enum Received {
    /// The relay challenged the connection, which answered.
    Authenticated,
    Message(Value),
}

/// One websocket to one relay.
struct Connection<'a> {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    relay: String,
    secret: &'a SecretKey,
    now: i64,
    authenticated: bool,
}

impl<'a> Connection<'a> {
    async fn open(relay: &str, secret: &'a SecretKey, now: i64) -> Result<Self> {
        let (socket, _) =
            tokio_tungstenite::connect_async_tls_with_config(relay, None, false, Some(tls()))
                .await
                .map_err(|error| anyhow!("couldn't connect to {relay}: {error}"))?;
        Ok(Self {
            socket,
            relay: relay.to_string(),
            secret,
            now,
            authenticated: false,
        })
    }

    async fn send(&mut self, message: &Value) -> Result<()> {
        self.socket
            .send(Message::Text(message.to_string().into()))
            .await?;
        Ok(())
    }

    /// The relay's next message, answering a challenge itself.
    async fn receive(&mut self) -> Result<Received> {
        loop {
            let frame = self
                .socket
                .next()
                .await
                .ok_or_else(|| anyhow!("{} closed the connection", self.relay))??;
            let Message::Text(text) = frame else {
                continue;
            };
            let Ok(message) = serde_json::from_str::<Value>(&text) else {
                continue;
            };

            if message[0] == "AUTH"
                && let Some(challenge) = message[1].as_str()
            {
                let answer = self.answer(challenge);
                self.send(&json!(["AUTH", answer])).await?;
                self.authenticated = true;
                return Ok(Received::Authenticated);
            }
            return Ok(Received::Message(message));
        }
    }

    /// A NIP-42 answer to a challenge on this relay.
    fn answer(&self, challenge: &str) -> Event {
        let hashed = EventContent::new()
            .with_tags(
                Tags::new()
                    .add("relay", [self.relay.as_str()])
                    .add("challenge", [challenge]),
            )
            .with_kind(CLIENT_AUTH)
            .with_created_at(self.now)
            .with_pubkey(self.secret.public_key())
            .with_id();
        let sig = self.secret.sign(&hashed.id.to_bytes());
        hashed.with_sig(sig)
    }

    async fn close(mut self) {
        let _ = self.socket.close(None).await;
    }
}

/// TLS for `wss://` relays, trusting the Mozilla roots.
fn tls() -> Connector {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("ring supports the default protocol versions")
    .with_root_certificates(roots)
    .with_no_client_auth();
    Connector::Rustls(Arc::new(config))
}
