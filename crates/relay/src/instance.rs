//! One relay as the runtime serves it: its settings and access snapshot, its open
//! connections, and what it does with each message a client sends. Reloading a
//! relay swaps its snapshot and leaves its connections open.

use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use caravel_core::access::{self, MemberSource};
use caravel_core::crypto::RelayKey;
use caravel_core::events::{self, DELETION, Saved, StoredEvent, VANISH};
use caravel_core::relay_policy::{self, EventFacts, Join, Refusal, RelaySnapshot, kinds};
use caravel_core::relays::Relay;
use caravel_core::{Core, Error};
use caravel_core::{notifications, rooms};
use coracle_lib::events::{Event, EventId};
use coracle_lib::filters::Filter;
use coracle_lib::sync::{FrameBudget, Item, Message as SyncMessage, SyncSet, reconcile_responder};
use tokio_util::sync::CancellationToken;

use crate::RelayLimits;
use crate::auth;
use crate::connections::{Connections, RegisterError};
use crate::message::{
    RawEvent, RelayMessage, SubscriptionId, refused_event, refused_reconciliation,
    refused_subscription,
};
use crate::push::{self, Pusher};
use crate::session::Session;

/// The size a reconciliation round is held to. Negentropy folds what doesn't fit
/// into a fingerprint and carries on next round.
const SYNC_FRAME_BYTES: usize = 60_000;

/// A relay's row, its key, and the snapshot its rules read, replaced together
/// on reload.
pub struct InstanceState {
    pub relay: Relay,
    pub key: RelayKey,
    pub snapshot: RelaySnapshot,
    /// Its push subscriptions, when push is on.
    pub push: Vec<push::Subscription>,
}

impl InstanceState {
    pub async fn load(core: &Core, relay: Relay) -> caravel_core::Result<Self> {
        let push = if relay.push_enabled {
            push::load_subscriptions(core, &relay.id).await?
        } else {
            Vec::new()
        };

        Ok(Self {
            key: RelayKey::decrypt(&core.config.platform_key, &relay.secret)?,
            snapshot: relay_policy::load_snapshot(core.db.pool(), &relay).await?,
            push,
            relay,
        })
    }
}

pub struct RelayInstance {
    core: Core,
    limits: RelayLimits,
    state: RwLock<Arc<InstanceState>>,
    /// Held while a reload reads the database, so a slower reload can't replace a
    /// newer state with an older one.
    reloading: tokio::sync::Mutex<()>,
    connections: Connections,
    pusher: Pusher,
    /// Cancelled when the relay stops being served, closing its connections.
    closed: CancellationToken,
}

impl RelayInstance {
    pub fn new(
        core: Core,
        limits: RelayLimits,
        state: InstanceState,
        pusher: Pusher,
        closed: CancellationToken,
    ) -> Self {
        Self {
            core,
            limits,
            state: RwLock::new(Arc::new(state)),
            reloading: tokio::sync::Mutex::new(()),
            connections: Connections::default(),
            pusher,
            closed,
        }
    }

    pub fn core(&self) -> &Core {
        &self.core
    }

    pub fn relay_id(&self) -> String {
        self.state().relay.id.clone()
    }

    pub fn state(&self) -> Arc<InstanceState> {
        self.state
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// Reload the relay's state from its row, keeping its connections open.
    pub async fn reload(&self, relay: Relay) -> caravel_core::Result<()> {
        let _reloading = self.reloading.lock().await;
        let state = InstanceState::load(&self.core, relay).await?;
        *self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Arc::new(state);
        Ok(())
    }

    /// Reload the relay's access lists and rooms after a change to them.
    pub async fn refresh(&self) {
        if let Err(error) = self.reload(self.state().relay.clone()).await {
            tracing::error!(relay = %self.relay_id(), error = %error, "failed to refresh relay");
        }
    }

    pub fn limits(&self) -> &RelayLimits {
        &self.limits
    }

    /// Cancelled once the relay is no longer served.
    pub fn closed(&self) -> CancellationToken {
        self.closed.clone()
    }

    /// Stop serving: every open connection closes.
    pub fn close(&self) {
        self.closed.cancel();
    }

    /// The checks that settle an event before its signature is verified.
    pub fn screen(&self, event: &Event) -> Result<(), Refusal> {
        relay_policy::screen(event, self.core.clock.now())
    }

    /// Challenge every connection as it opens, so a reader can prove itself before
    /// anything is refused.
    pub async fn on_connect(&self, session: &Session) {
        let challenge = auth::challenge();
        self.connections.open(session, challenge.clone());
        session.send(RelayMessage::Auth { challenge }).await;
    }

    pub fn on_disconnect(&self, session: &Session) {
        self.connections.close(session);
    }

    pub async fn on_event(&self, session: &Session, event: Event) {
        match self.ingest(session, &event).await {
            Ok(published) => {
                session.send(RelayMessage::accepted(event.id)).await;
                self.fanout(&event);
                for event in &published {
                    self.fanout(event);
                }

                let state = self.state();
                if state.relay.push_enabled {
                    self.pusher
                        .push(&state.snapshot, &state.push, &state.relay.host(), &event);
                }
            }
            Err(refusal) => self.refuse_event(session, event.id, refusal).await,
        }
    }

    /// Take an event from a client: admit it, then act on it, keep it, or only pass
    /// it on if it is ephemeral. Whatever the event changes is applied in the
    /// transaction that stores it, and the relay's snapshot is reloaded before the
    /// client hears back, so its next message meets the new state. Returns the
    /// events the relay published about the change.
    async fn ingest(&self, session: &Session, event: &Event) -> Result<Vec<Event>, Refusal> {
        let state = self.state();
        let authed = self.connections.pubkeys(session);
        let now = self.core.clock.now();

        relay_policy::admit_write(&state.snapshot, event, &authed, now)?;

        if !events::is_storable(event) {
            return Err(Refusal::invalid("event contains a NUL character"));
        }

        if event.kind == kinds::PUSH_SUBSCRIPTION {
            let hosts = [state.relay.canonical_host(), state.relay.host()];
            relay_policy::check_push_subscription(
                event,
                &hosts,
                push::held_by(&state.push, event),
            )?;
            // A callback naming a local address outright is refused now for a clear
            // answer; delivery refuses every local address however it is named.
            let callback = event.tags.value("callback").unwrap_or_default();
            if self.core.outbound.check(callback).is_err() {
                return Err(Refusal::invalid(
                    "callback must not point to a local or internal address",
                ));
            }
        }

        let published = match event.kind {
            kinds::RELAY_JOIN => self.join(&state, event, now).await?,
            kinds::RELAY_LEAVE => self.leave(&state, event, now).await,
            kind if !relay_policy::is_stored(kind) => Vec::new(),
            _ => self.store(&state, event, now).await?,
        };

        let changes_push = state.relay.push_enabled
            && matches!(event.kind, kinds::PUSH_SUBSCRIPTION | DELETION | VANISH);
        if !published.is_empty() || rooms::is_moderation_kind(event.kind) || changes_push {
            self.refresh().await;
        }

        Ok(published)
    }

    /// Admit a NIP-43 join request: a member already, anyone on an open relay, or
    /// the holder of a live claim, within the plan's member limit. A join is logged
    /// against any claim it presents.
    async fn join(
        &self,
        state: &InstanceState,
        event: &Event,
        now: i64,
    ) -> Result<Vec<Event>, Refusal> {
        let relay_id = state.relay.id.as_str();
        let pubkey = event.pubkey.to_hex();

        let source = match relay_policy::admit_join(&state.snapshot, event)? {
            Join::AlreadyMember => None,
            Join::Open => Some(MemberSource::PublicJoin),
            Join::Claim(code) => {
                let live = access::is_live_claim(self.core.db.pool(), relay_id, &code)
                    .await
                    .map_err(|_| Refusal::Error("the invite code could not be checked"))?;
                if !live {
                    return Err(Refusal::restricted("failed to validate invite code"));
                }
                Some(MemberSource::Claim)
            }
        };

        let presented = event.tags.value("claim");
        let limit = state.snapshot.member_limit;
        let result = self
            .core
            .db
            .with_tx(async |conn| {
                let published = match source {
                    Some(source) => {
                        access::admit_within_limit(
                            &mut *conn, &state.key, relay_id, &pubkey, source, limit, now,
                        )
                        .await?
                    }
                    None => Vec::new(),
                };
                if let Some(code) = presented {
                    access::record_claim_use(conn, relay_id, code, &pubkey, now).await?;
                }
                Ok::<_, Error>(published)
            })
            .await;

        match result {
            Ok(published) => Ok(published),
            Err(Error::Conflict { message, .. }) => Err(Refusal::restricted(message)),
            Err(error) => {
                tracing::warn!(relay = relay_id, error = %error, "failed to admit a member");
                Err(Refusal::Error("the join could not be applied"))
            }
        }
    }

    /// Act on a NIP-43 leave request. The owner can't leave, which is no error.
    async fn leave(&self, state: &InstanceState, event: &Event, now: i64) -> Vec<Event> {
        let relay = &state.relay;
        let pubkey = event.pubkey.to_hex();
        let result = self
            .core
            .db
            .with_tx(async |conn| {
                access::remove(
                    conn,
                    &state.key,
                    &relay.id,
                    &relay.tenant_pubkey,
                    &pubkey,
                    now,
                )
                .await
            })
            .await;

        result.unwrap_or_default()
    }

    /// Keep an event, applying a deletion, a request to vanish, or a change to a
    /// room in the same transaction.
    async fn store(
        &self,
        state: &InstanceState,
        event: &Event,
        now: i64,
    ) -> Result<Vec<Event>, Refusal> {
        if event.kind == VANISH && !self.is_addressed(&state.relay, event) {
            return Err(Refusal::invalid(
                "this request to vanish names another relay",
            ));
        }

        let relay_id = state.relay.id.as_str();
        let rooms_enabled = state.snapshot.groups_enabled;
        let (saved, published) = self
            .core
            .db
            .with_tx(async |conn| {
                let saved = events::save_event(&mut *conn, relay_id, event, now).await?;
                let mut published = Vec::new();
                if saved == Saved::New {
                    match event.kind {
                        DELETION => {
                            events::apply_deletion(conn, relay_id, event).await?;
                        }
                        VANISH => {
                            events::apply_vanish(
                                conn,
                                relay_id,
                                &event.pubkey.as_bytes(),
                                event.created_at,
                            )
                            .await?;
                        }
                        kind if rooms_enabled
                            && rooms::is_room_event(kind, rooms::room_id(event)) =>
                        {
                            published =
                                rooms::apply_event(conn, &state.key, relay_id, event, now).await?;
                        }
                        _ => {}
                    }
                }
                Ok::<_, Error>((saved, published))
            })
            .await
            .map_err(|error| {
                tracing::warn!(relay = relay_id, error = %error, "failed to store event");
                Refusal::Error("the event could not be stored")
            })?;

        match saved {
            Saved::New => Ok(published),
            Saved::Duplicate => Err(Refusal::Duplicate),
            Saved::Deleted => Err(Refusal::blocked("this event was deleted")),
        }
    }

    /// Whether a NIP-62 request names this relay, by one of its hosts or as all.
    fn is_addressed(&self, relay: &Relay, event: &Event) -> bool {
        let hosts = [relay.canonical_host(), relay.custom_domain.clone()];
        event.tags.find_all("relay").any(|tag| {
            tag.value() == "ALL_RELAYS" || hosts.contains(&auth::relay_host(tag.value()))
        })
    }

    /// Send an event to every subscription that matches it and whose connection
    /// may still read the relay and see the event, without signatures where the
    /// relay strips them.
    pub fn fanout(&self, event: &Event) {
        let state = self.state();
        let author = event.pubkey.to_hex();
        let facts = EventFacts::of(event, &author);
        let stored = StoredEvent::from_event(event);
        let mut rendered: [Option<RawEvent>; 2] = [None, None];

        for matched in self.connections.matching(event) {
            // A reader banned or removed since subscribing stops receiving at once.
            if relay_policy::admit_req(&state.snapshot, &matched.pubkeys).is_err()
                || !relay_policy::can_any_receive(&state.snapshot, &matched.pubkeys, facts)
            {
                continue;
            }

            let strip = relay_policy::strips_signature(&state.snapshot, &matched.pubkeys);
            let raw = rendered[usize::from(strip)]
                .get_or_insert_with(|| {
                    RawEvent::from_json(stored.to_json(strip)).expect("events render as json")
                })
                .clone();
            let message = RelayMessage::Event {
                subscription_id: matched.subscription_id,
                event: raw,
            };

            // A reader whose queue is full doesn't hold up everyone else.
            if !matched.session.try_send(message) {
                tracing::debug!(
                    session = matched.session.id(),
                    "dropped an event for a slow reader"
                );
            }
        }
    }

    pub async fn on_req(
        &self,
        session: &Session,
        subscription_id: SubscriptionId,
        filters: Vec<Filter>,
    ) {
        if let Err(refusal) = self.read(session, &subscription_id, filters).await {
            self.offer_challenge(session, &refusal).await;
            session
                .send(refused_subscription(subscription_id, &refusal))
                .await;
        }
    }

    /// Subscribe, then send the stored events each filter matches that the
    /// connection may see, then `EOSE`. Subscribing first means an event that
    /// arrives mid-query isn't missed.
    async fn read(
        &self,
        session: &Session,
        subscription_id: &SubscriptionId,
        filters: Vec<Filter>,
    ) -> Result<(), Refusal> {
        let state = self.state();
        let authed = self.connections.pubkeys(session);
        let now = self.core.clock.now();

        relay_policy::admit_req(&state.snapshot, &authed)?;

        self.connections
            .subscribe(
                session,
                subscription_id.clone(),
                filters.clone(),
                self.limits.max_subscriptions,
            )
            .map_err(|error| {
                registration_refusal(
                    error,
                    format!(
                        "at most {} subscriptions may be open at once",
                        self.limits.max_subscriptions
                    ),
                )
            })?;

        let strip = relay_policy::strips_signature(&state.snapshot, &authed);
        let mut sent = HashSet::new();

        for filter in &filters {
            let found = events::query_events(
                self.core.db.pool(),
                &state.relay.id,
                filter,
                self.limits.max_limit,
                now,
            )
            .await
            .map_err(|error| {
                tracing::warn!(relay = %state.relay.id, error = %error, "query failed");
                self.connections.unsubscribe(session, subscription_id);
                Refusal::Error("the query could not be run")
            })?;

            for stored in found {
                let author = stored.head.pubkey_hex();
                if !sent.insert(stored.head.id)
                    || !relay_policy::can_any_receive(
                        &state.snapshot,
                        &authed,
                        EventFacts::of_head(&stored.head, &author),
                    )
                {
                    continue;
                }

                let message = RelayMessage::Event {
                    subscription_id: subscription_id.clone(),
                    event: RawEvent::from_json(stored.to_json(strip))
                        .expect("events render as json"),
                };
                if !session.send(message).await {
                    return Ok(());
                }
            }
        }

        session
            .send(RelayMessage::Eose(subscription_id.clone(), vec![]))
            .await;
        Ok(())
    }

    pub fn on_close(&self, session: &Session, subscription_id: &SubscriptionId) {
        self.connections.unsubscribe(session, subscription_id);
    }

    pub async fn on_count(&self, session: &Session, subscription_id: SubscriptionId) {
        let refusal = Refusal::blocked("this relay does not count");
        session
            .send(refused_subscription(subscription_id, &refusal))
            .await;
    }

    pub async fn on_auth(&self, session: &Session, event: Event) {
        let Some(challenge) = self.connections.challenge(session) else {
            self.refuse_event(
                session,
                event.id,
                Refusal::Error("this connection has no challenge outstanding"),
            )
            .await;
            return;
        };

        let pubkey = match auth::validate(&event, &challenge, session.host(), self.core.clock.now())
        {
            Ok(pubkey) => pubkey,
            Err(reason) => {
                self.refuse_event(session, event.id, Refusal::invalid(reason))
                    .await;
                return;
            }
        };

        if let Err(error) =
            self.connections
                .authenticate(session, pubkey.clone(), self.limits.max_identities)
        {
            let refusal = registration_refusal(
                error,
                format!(
                    "at most {} pubkeys may be proved on one connection",
                    self.limits.max_identities
                ),
            );
            self.refuse_event(session, event.id, refusal).await;
            return;
        }

        session.send(RelayMessage::accepted(event.id)).await;

        // A message written while the pubkey was away reaches it as it returns.
        let relay_id = self.relay_id();
        match notifications::get_notification(
            self.core.db.pool(),
            &relay_id,
            &pubkey,
            self.core.clock.now(),
        )
        .await
        {
            Ok(Some(notification)) => {
                session
                    .send(RelayMessage::Notify(notification.message))
                    .await;
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(relay = %relay_id, error = %error, "failed to read a notification")
            }
        }
    }

    /// Send a message to every connection authenticated as a pubkey.
    pub fn deliver_notification(&self, pubkey: &str, message: &str) {
        for session in self.connections.sessions_for(pubkey) {
            session.try_send(RelayMessage::Notify(message.to_string()));
        }
    }

    pub async fn on_neg_open(
        &self,
        session: &Session,
        subscription_id: SubscriptionId,
        filter: Filter,
        message: String,
    ) {
        let result = match self
            .open_reconciliation(session, &subscription_id, filter)
            .await
        {
            Ok(set) => {
                self.reconcile(session, &subscription_id, set, &message)
                    .await
            }
            Err(refusal) => Err(refusal),
        };

        if let Err(refusal) = result {
            self.offer_challenge(session, &refusal).await;
            session
                .send(refused_reconciliation(subscription_id, &refusal))
                .await;
        }
    }

    pub async fn on_neg_msg(
        &self,
        session: &Session,
        subscription_id: SubscriptionId,
        message: String,
    ) {
        let result = match self.connections.reconciliation(session, &subscription_id) {
            Some(set) => {
                self.reconcile(session, &subscription_id, set, &message)
                    .await
            }
            None => Err(Refusal::Closed("no reconciliation is open under that id")),
        };

        if let Err(refusal) = result {
            session
                .send(refused_reconciliation(subscription_id, &refusal))
                .await;
        }
    }

    pub fn on_neg_close(&self, session: &Session, subscription_id: &SubscriptionId) {
        self.connections
            .close_reconciliation(session, subscription_id);
    }

    /// The set of events matching a filter that the connection may see, as of now.
    async fn open_reconciliation(
        &self,
        session: &Session,
        subscription_id: &SubscriptionId,
        filter: Filter,
    ) -> Result<Arc<SyncSet>, Refusal> {
        let state = self.state();
        let authed = self.connections.pubkeys(session);

        relay_policy::admit_req(&state.snapshot, &authed)?;

        let heads = events::query_event_heads(
            self.core.db.pool(),
            &state.relay.id,
            &filter,
            self.limits.max_sync,
            self.core.clock.now(),
        )
        .await
        .map_err(|error| {
            tracing::warn!(relay = %state.relay.id, error = %error, "sync scan failed");
            Refusal::Error("the set could not be read")
        })?
        .ok_or_else(|| {
            Refusal::blocked(format!(
                "this relay reconciles at most {} events at a time",
                self.limits.max_sync
            ))
        })?;

        let items = heads
            .into_iter()
            .filter(|head| {
                let author = head.pubkey_hex();
                relay_policy::can_any_receive(
                    &state.snapshot,
                    &authed,
                    EventFacts::of_head(head, &author),
                )
            })
            .map(|head| Item {
                timestamp: head.created_at,
                id: EventId::new(head.id),
            });
        let set = Arc::new(SyncSet::from_items(items));

        self.connections
            .open_reconciliation(
                session,
                subscription_id.clone(),
                set.clone(),
                self.limits.max_reconciliations,
            )
            .map_err(|error| {
                registration_refusal(
                    error,
                    format!(
                        "at most {} reconciliations may be open at once",
                        self.limits.max_reconciliations
                    ),
                )
            })?;

        Ok(set)
    }

    async fn reconcile(
        &self,
        session: &Session,
        subscription_id: &SubscriptionId,
        set: Arc<SyncSet>,
        message: &str,
    ) -> Result<(), Refusal> {
        let incoming = SyncMessage::from_hex(message).map_err(|error| {
            self.connections
                .close_reconciliation(session, subscription_id);
            Refusal::invalid(error.to_string())
        })?;

        // Fingerprinting a large set is too slow for an async worker thread.
        let reply = tokio::task::spawn_blocking(move || {
            reconcile_responder(&*set, &incoming, FrameBudget::bytes(SYNC_FRAME_BYTES)).to_hex()
        })
        .await
        .map_err(|_| Refusal::Error("the reconciliation failed"))?;

        session
            .send(RelayMessage::NegMsg {
                subscription_id: subscription_id.clone(),
                message: reply,
            })
            .await;
        Ok(())
    }

    async fn refuse_event(&self, session: &Session, event_id: EventId, refusal: Refusal) {
        self.offer_challenge(session, &refusal).await;
        session.send(refused_event(event_id, &refusal)).await;
    }

    /// Re-offer the connection's challenge alongside a refusal that asks for one,
    /// so a client that missed the first `AUTH` needn't reconnect.
    async fn offer_challenge(&self, session: &Session, refusal: &Refusal) {
        if refusal.needs_auth()
            && let Some(challenge) = self.connections.challenge(session)
        {
            session.send(RelayMessage::Auth { challenge }).await;
        }
    }
}

fn registration_refusal(error: RegisterError, too_many: String) -> Refusal {
    match error {
        RegisterError::TooMany => Refusal::rate_limited(too_many),
        RegisterError::Gone => Refusal::Error("the connection is closing"),
    }
}
