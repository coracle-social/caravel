//! A client for other people's Nostr relays: publishing events to them, reading
//! events from them, and asking a question an event answers, as NIP-47 wallets
//! are asked. Each call opens its own connections and closes them when it is
//! done, and a connection answers a relay's NIP-42 challenge with its key.
//!
//! It also carries [`Key`], the workspace's general-purpose Nostr key: the
//! platform's identity, a relay's, and the key the platform encrypts its stored
//! secrets to itself with.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use coracle_lib::events::{Event, EventContent};
use coracle_lib::filters::Filter;
use coracle_lib::keys::{PublicKey, SecretKey};
use coracle_lib::tags::{Tag, Tags};
use futures_util::future::{join_all, select_ok};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::time::{Instant, timeout_at};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

use crate::outbound;

pub mod kinds;

/// The kind a client answers a relay's challenge with.
const CLIENT_AUTH: u16 = 22242;

/// A Nostr key: the platform's identity, a relay's, or anyone's. It encrypts to
/// itself with NIP-44 and signs the events its holder publishes.
#[derive(Clone)]
pub struct Key {
    secret: SecretKey,
    pubkey: PublicKey,
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Key")
            .field("pubkey", &self.pubkey.to_hex())
            .finish()
    }
}

impl Key {
    pub fn from_hex(secret: &str) -> Result<Self> {
        let secret = SecretKey::from_hex(secret).map_err(|e| anyhow!("invalid secret key: {e}"))?;
        let pubkey = secret.public_key();
        Ok(Self { secret, pubkey })
    }

    pub fn pubkey(&self) -> String {
        self.pubkey.to_hex()
    }

    pub fn secret_hex(&self) -> String {
        self.secret.to_hex()
    }

    /// NIP-44 encrypt `plaintext` to this key's holder.
    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        self.secret
            .nip44_encrypt(&self.pubkey, plaintext)
            .map_err(|e| anyhow!("encryption failed: {e}"))
    }

    pub fn decrypt(&self, ciphertext: &str) -> Result<String> {
        self.secret
            .nip44_decrypt(&self.pubkey, ciphertext)
            .map_err(|e| anyhow!("decryption failed: {e}"))
    }

    /// A signed event from this key.
    pub fn sign(&self, kind: u16, tags: Vec<Vec<String>>, content: &str, created_at: i64) -> Event {
        let tags: Tags = tags.into_iter().map(Tag).collect();
        let hashed = EventContent::new()
            .with_content(content)
            .with_tags(tags)
            .with_kind(kind)
            .with_created_at(created_at)
            .with_pubkey(self.secret.public_key())
            .with_id();
        let sig = self.secret.sign(hashed.id.as_bytes());
        hashed.with_sig(sig)
    }
}

/// A fresh keypair as hex `(pubkey, secret)`.
pub fn generate_keypair() -> (String, String) {
    let secret = SecretKey::generate();
    (secret.public_key().to_hex(), secret.to_hex())
}

/// A key to talk to relays as, refusing relays on local or internal addresses.
#[derive(Clone)]
pub struct Client {
    secret: SecretKey,
    allow_private: bool,
}

impl Client {
    pub fn new(secret: SecretKey) -> Self {
        Self {
            secret,
            allow_private: false,
        }
    }

    /// A client that also reaches local addresses, for tests' relays on loopback.
    pub fn allowing_private(secret: SecretKey) -> Self {
        Self {
            secret,
            allow_private: true,
        }
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
                .map(|relay| self.fetch_from(relay, filter, now, timeout)),
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
        let mut connection = Connection::open(relay, &self.secret, now, self.allow_private).await?;
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

    /// The events a relay sends until EOSE or the timeout, failing if it sent none
    /// by the timeout.
    async fn fetch_from(
        &self,
        relay: &str,
        filter: &Filter,
        now: i64,
        timeout: Duration,
    ) -> Result<Vec<Event>> {
        let deadline = Instant::now() + timeout;
        let open = Connection::open(relay, &self.secret, now, self.allow_private);
        let mut connection = within(timeout, open).await?;
        let id = hex::encode(rand::random::<[u8; 16]>());
        let subscribe = json!(["REQ", id, filter]);
        connection.send(&subscribe).await?;

        let mut events = Vec::new();
        let mut retry = Retry::default();
        loop {
            let Ok(received) = timeout_at(deadline, connection.receive()).await else {
                if events.is_empty() {
                    bail!("the relay didn't answer in time");
                }
                break;
            };
            match received? {
                Received::Authenticated => {
                    if retry.on_authenticated() {
                        connection.send(&subscribe).await?;
                    }
                }
                Received::Message(message) if message[1] != id => {}
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

        connection.send(&json!(["CLOSE", id])).await?;
        connection.close().await;
        Ok(events)
    }

    /// Publish `event` once a subscription to `reply` is open, since an answer
    /// sent before then is missed, and wait for the first verified answer.
    async fn request_from(
        &self,
        relay: &str,
        event: &Event,
        reply: &Filter,
        now: i64,
    ) -> Result<Event> {
        let mut connection = Connection::open(relay, &self.secret, now, self.allow_private).await?;
        let id = hex::encode(rand::random::<[u8; 16]>());
        let subscribe = json!(["REQ", id, reply]);
        let publish = json!(["EVENT", event]);
        connection.send(&subscribe).await?;

        let mut published = false;
        let mut subscribe_retry = Retry::default();
        let mut publish_retry = Retry::default();
        loop {
            match connection.receive().await? {
                Received::Authenticated => {
                    if subscribe_retry.on_authenticated() {
                        connection.send(&subscribe).await?;
                    }
                    if publish_retry.on_authenticated() {
                        connection.send(&publish).await?;
                    }
                }
                Received::Message(message) => match message[0].as_str() {
                    Some("EOSE") if message[1] == id && !published => {
                        published = true;
                        connection.send(&publish).await?;
                    }
                    Some("EVENT") if message[1] == id => {
                        if let Some(answer) = verified(&message[2])
                            && reply.matches(&answer)
                        {
                            connection.close().await;
                            return Ok(answer);
                        }
                    }
                    Some("OK") if message[1] == event.id.to_hex() && message[2] != true => {
                        let reason = message[3].as_str().unwrap_or_default();
                        if publish_retry.on_refused(reason, connection.authenticated) {
                            connection.send(&publish).await?;
                        } else if !publish_retry.waiting {
                            bail!("{relay} refused the request: {reason}");
                        }
                    }
                    Some("CLOSED") if message[1] == id => {
                        let reason = message[2].as_str().unwrap_or_default();
                        if subscribe_retry.on_refused(reason, connection.authenticated) {
                            connection.send(&subscribe).await?;
                        } else if !subscribe_retry.waiting {
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
    /// A websocket to `relay`, refusing local and internal addresses unless
    /// `allow_private`.
    async fn open(
        relay: &str,
        secret: &'a SecretKey,
        now: i64,
        allow_private: bool,
    ) -> Result<Self> {
        let url: url::Url = relay.parse()?;
        let (socket, _) = async {
            let stream = outbound::connect(&url, allow_private).await?;
            let tls = Some(tls());
            Ok(tokio_tungstenite::client_async_tls_with_config(relay, stream, None, tls).await?)
        }
        .await
        .map_err(|error: anyhow::Error| anyhow!("couldn't connect to {relay}: {error}"))?;
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use tokio::net::TcpListener;

    /// A relay that answers a REQ with `events` and never sends EOSE, and its url.
    async fn stalling_relay(events: Vec<Event>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            while let Some(Ok(Message::Text(text))) = socket.next().await {
                let message: Value = serde_json::from_str(&text).unwrap();
                if message[0] != "REQ" {
                    continue;
                }
                for event in &events {
                    let reply = json!(["EVENT", message[1], event]);
                    let _ = socket.send(Message::Text(reply.to_string().into())).await;
                }
            }
        });
        url
    }

    /// A relay that takes events from anyone but opens subscriptions only once the
    /// connection has authenticated, answering each event with `answer` on every
    /// open subscription, as a relay forwards an ephemeral event; and its url.
    async fn auth_to_read_relay(answer: Event) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let text = |message: Value| Message::Text(message.to_string().into());
            let _ = socket.send(text(json!(["AUTH", "challenge"]))).await;
            let (mut authenticated, mut open) = (false, Vec::new());
            while let Some(Ok(Message::Text(received))) = socket.next().await {
                let message: Value = serde_json::from_str(&received).unwrap();
                let mut replies = Vec::new();
                match message[0].as_str() {
                    Some("AUTH") => {
                        authenticated = true;
                        replies.push(json!(["OK", message[1]["id"], true, ""]));
                    }
                    Some("REQ") if !authenticated => {
                        replies.push(json!(["CLOSED", message[1], "auth-required: sign in"]));
                    }
                    Some("REQ") => {
                        open.push(message[1].clone());
                        replies.push(json!(["EOSE", message[1]]));
                    }
                    Some("EVENT") => {
                        replies.push(json!(["OK", message[1]["id"], true, ""]));
                        replies.extend(open.iter().map(|id| json!(["EVENT", id, answer])));
                    }
                    _ => {}
                }
                for reply in replies {
                    let _ = socket.send(text(reply)).await;
                }
            }
        });
        url
    }

    fn note(secret: &SecretKey) -> Event {
        let hashed = EventContent::new()
            .with_content("hello")
            .with_kind(1)
            .with_created_at(0)
            .with_pubkey(secret.public_key())
            .with_id();
        let sig = secret.sign(&hashed.id.to_bytes());
        hashed.with_sig(sig)
    }

    #[tokio::test]
    async fn a_relay_without_eose_by_the_timeout_keeps_what_it_sent() {
        let secret = SecretKey::generate();
        let event = note(&secret);
        let relays = [stalling_relay(vec![event.clone()]).await];
        let fetched = Client::allowing_private(secret)
            .fetch(&relays, &Filter::default(), 0, Duration::from_millis(200))
            .await
            .unwrap();
        assert_eq!(fetched, [event]);
    }

    #[tokio::test]
    async fn a_relay_that_sent_nothing_by_the_timeout_did_not_answer() {
        let relays = [stalling_relay(Vec::new()).await];
        let fetched = Client::allowing_private(SecretKey::generate())
            .fetch(&relays, &Filter::default(), 0, Duration::from_millis(200))
            .await;
        assert!(fetched.is_err());
    }

    #[tokio::test]
    async fn a_relay_on_a_local_address_is_never_dialed() {
        let secret = SecretKey::generate();
        for host in ["127.0.0.1", "localhost"] {
            let url = stalling_relay(vec![note(&secret)]).await;
            let relays = [url.replace("127.0.0.1", host)];
            let error = Client::new(secret.clone())
                .fetch(&relays, &Filter::default(), 0, Duration::from_secs(2))
                .await
                .unwrap_err();
            assert!(error.to_string().contains("no public address"), "{error}");
        }
    }

    #[tokio::test]
    async fn a_request_subscribes_again_after_auth_before_it_publishes() {
        let (asker, answerer) = (SecretKey::generate(), SecretKey::generate());
        let answer = note(&answerer);
        let relays = [auth_to_read_relay(answer.clone()).await];
        let reply = Filter {
            authors: Some(BTreeSet::from([answerer.public_key()])),
            ..Default::default()
        };
        let answered = Client::allowing_private(asker.clone())
            .request(&relays, &note(&asker), &reply, 0, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(answered, answer);
    }
}
