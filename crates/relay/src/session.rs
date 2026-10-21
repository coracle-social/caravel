//! One client connection, from the upgrade to the close. The read loop parses
//! what arrives and hands it to the relay instance; a write task owns the socket
//! and drains a queue, so anything holding a [`Session`] can push a message at
//! the client without waiting on the socket.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message as WsMessage, WebSocket};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::{Instant, interval_at};
use tokio_util::sync::CancellationToken;

use caravel_core::relay_policy::Refusal;

use crate::instance::RelayInstance;
use crate::message::{
    ClientMessage, RelayMessage, SubscriptionId, refused_event, refused_reconciliation,
    refused_subscription,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// The close code a session ends with when the server is going away.
const GOING_AWAY: u16 = 1001;

/// How connections are carried.
#[derive(Clone, Copy, Debug)]
pub struct Transport {
    /// The largest websocket message the relay decodes, in bytes.
    pub max_message_size: usize,
    /// How many outbound messages may queue for one client.
    pub send_buffer: usize,
    /// How often to ping a client, to notice one that went away silently.
    pub ping_interval: Duration,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            max_message_size: 512 * 1024,
            send_buffer: 256,
            ping_interval: Duration::from_secs(30),
        }
    }
}

/// A handle to one connected client. Cloning is cheap, and sending to a client
/// that has gone away fails rather than blocking.
#[derive(Debug, Clone)]
pub struct Session {
    id: u64,
    host: String,
    sender: mpsc::Sender<RelayMessage>,
}

impl Session {
    /// This connection's number, unique within the process.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The host the client connected to, which its NIP-42 answers name.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Queue a message, waiting while the client's queue is full. False once the
    /// connection is gone.
    pub async fn send(&self, message: RelayMessage) -> bool {
        self.sender.send(message).await.is_ok()
    }

    /// Queue a message without waiting, dropping it if the queue is full, for
    /// places one slow client mustn't hold up.
    pub fn try_send(&self, message: RelayMessage) -> bool {
        self.sender.try_send(message).is_ok()
    }
}

/// Drive one connection until it closes or the server shuts down.
pub async fn run(
    socket: WebSocket,
    addr: SocketAddr,
    host: String,
    instance: Arc<RelayInstance>,
    transport: Transport,
    shutdown: CancellationToken,
) {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (sink, stream) = socket.split();
    let (sender, receiver) = mpsc::channel(transport.send_buffer);
    let session = Session { id, host, sender };

    tracing::debug!(session = id, %addr, relay = %instance.relay_id(), "connected");

    let writer = tokio::spawn(write(sink, receiver, transport, shutdown.clone()));

    instance.on_connect(&session).await;
    tokio::select! {
        _ = read(&session, &instance, stream) => {}
        _ = shutdown.cancelled() => {}
    }
    instance.on_disconnect(&session);

    tracing::debug!(session = id, "disconnected");

    // The writer ends when every `Session` is dropped or the server shuts down.
    drop(session);
    let _ = writer.await;
}

async fn read(session: &Session, instance: &RelayInstance, mut stream: SplitStream<WebSocket>) {
    while let Some(frame) = stream.next().await {
        match frame {
            Ok(WsMessage::Text(text)) => dispatch(session, instance, text.as_str()).await,
            Ok(WsMessage::Binary(_)) => {
                let refusal = Refusal::invalid("nostr messages are text, not binary");
                session.send(RelayMessage::Notice(refusal.message())).await;
            }
            Ok(WsMessage::Close(_) | WsMessage::Ping(_) | WsMessage::Pong(_)) => {}
            Err(error) => {
                tracing::debug!(session = session.id(), %error, "read failed");
                break;
            }
        }
    }
}

/// Parse one message and hand it to the instance. Whether a message is
/// well-formed, and whether an event's signature is valid, are settled here.
async fn dispatch(session: &Session, instance: &RelayInstance, text: &str) {
    let message: ClientMessage = match serde_json::from_str(text) {
        Ok(message) => message,
        Err(error) => {
            session.send(malformed(text, &error)).await;
            return;
        }
    };

    match message {
        ClientMessage::Event(event) => {
            // Screened before the signature is checked, which costs far more.
            if let Err(refusal) = instance.screen(&event) {
                session.send(refused_event(event.id, &refusal)).await;
                return;
            }

            if let Err(error) = event.verify() {
                let refusal = Refusal::invalid(error.to_string());
                session.send(refused_event(event.id, &refusal)).await;
                return;
            }

            instance.on_event(session, event).await;
        }
        ClientMessage::Auth(event) => {
            if let Err(error) = event.verify() {
                let refusal = Refusal::invalid(error.to_string());
                session.send(refused_event(event.id, &refusal)).await;
                return;
            }

            instance.on_auth(session, event).await;
        }
        ClientMessage::Req {
            subscription_id,
            filters,
        } => instance.on_req(session, subscription_id, filters).await,
        ClientMessage::Close(subscription_id) => instance.on_close(session, &subscription_id),
        ClientMessage::Count {
            subscription_id,
            filters,
        } => instance.on_count(session, subscription_id, filters).await,
        ClientMessage::NegOpen {
            subscription_id,
            filter,
            message,
        } => {
            instance
                .on_neg_open(session, subscription_id, filter, message)
                .await
        }
        ClientMessage::NegMsg {
            subscription_id,
            message,
        } => instance.on_neg_msg(session, subscription_id, message).await,
        ClientMessage::NegClose(subscription_id) => {
            instance.on_neg_close(session, &subscription_id)
        }
    }
}

/// How to tell a client its message was no good: on the subscription it named,
/// where it named one, so a client waiting for an `EOSE` isn't left waiting.
fn malformed(text: &str, error: &serde_json::Error) -> RelayMessage {
    let refusal = Refusal::invalid(error.to_string());
    let Ok(parts) = serde_json::from_str::<Vec<&serde_json::value::RawValue>>(text) else {
        return RelayMessage::Notice(refusal.message());
    };

    let verb = parts
        .first()
        .and_then(|verb| serde_json::from_str::<String>(verb.get()).ok());
    let subscription_id = parts
        .get(1)
        .and_then(|id| serde_json::from_str::<SubscriptionId>(id.get()).ok());

    match (verb.as_deref(), subscription_id) {
        (Some("REQ" | "COUNT"), Some(id)) => refused_subscription(id, &refusal),
        (Some("NEG-OPEN" | "NEG-MSG"), Some(id)) => refused_reconciliation(id, &refusal),
        _ => RelayMessage::Notice(refusal.message()),
    }
}

/// Serialize queued messages onto the socket, pinging the client between them,
/// and say goodbye with 1001 when the server shuts down.
async fn write(
    mut sink: SplitSink<WebSocket, WsMessage>,
    mut receiver: mpsc::Receiver<RelayMessage>,
    transport: Transport,
    shutdown: CancellationToken,
) {
    let mut ping = interval_at(
        Instant::now() + transport.ping_interval,
        transport.ping_interval,
    );

    loop {
        let frame = tokio::select! {
            biased;
            _ = shutdown.cancelled() => {
                let _ = sink
                    .send(WsMessage::Close(Some(CloseFrame {
                        code: GOING_AWAY,
                        reason: "server shutting down".into(),
                    })))
                    .await;
                break;
            }
            message = receiver.recv() => match message {
                Some(message) => match serde_json::to_string(&message) {
                    Ok(json) => WsMessage::Text(json.into()),
                    Err(error) => {
                        tracing::warn!(verb = message.verb(), %error, "serialize failed");
                        continue;
                    }
                },
                None => break,
            },
            _ = ping.tick() => WsMessage::Ping(Vec::new().into()),
        };

        if sink.send(frame).await.is_err() {
            break;
        }
    }

    let _ = sink.close().await;
}
