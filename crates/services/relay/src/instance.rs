//! One relay as the runtime serves it: its settings and access snapshot, its open
//! connections, and what it does with each message a client sends. Reloading a
//! relay swaps its snapshot and leaves its connections open.

use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use caravel_billing::relays::Relay;
use caravel_core::{Core, ingest};
use caravel_relays::events::{Saved, StoredEvent};
use caravel_relays::policy::{EventFacts, Refusal, RelaySnapshot};
use caravel_util::error::Error;
use caravel_util::nostr::Key;
use caravel_util::nostr::kinds;
use caravel_util::nostr::kinds::{DELETION, VANISH};
use coracle_lib::events::{Event, EventId};
use coracle_lib::filters::Filter;
use coracle_lib::keys::PublicKey;
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
    pub key: Key,
    pub snapshot: RelaySnapshot,
    /// Its push subscriptions, when push is on.
    pub push: Vec<push::Subscription>,
    /// The tenant can take what the relay charges its members.
    pub accepting_payment: bool,
}

impl InstanceState {
    pub async fn load(core: &Core, relay: Relay) -> caravel_util::error::Result<Self> {
        let push = if relay.push_enabled {
            push::load_subscriptions(core, &relay.id).await?
        } else {
            Vec::new()
        };

        Ok(Self {
            key: core.relay_key(&relay)?,
            snapshot: caravel_core::events::load_snapshot(core, &relay).await?,
            push,
            accepting_payment: caravel_core::memberships::accepting_payment(core, &relay).await?,
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
    pub async fn reload(&self, relay: Relay) -> caravel_util::error::Result<()> {
        let _reloading = self.reloading.lock().await;
        self.install(relay).await
    }

    /// Reload the relay's access lists and rooms after a change to them. The row is
    /// read once the lock is held, so a settings reload in progress isn't undone.
    pub async fn refresh(&self) {
        let _reloading = self.reloading.lock().await;
        if let Err(error) = self.install(self.state().relay.clone()).await {
            tracing::error!(relay = %self.relay_id(), error = %error, "failed to refresh relay");
        }
    }

    /// Build the relay's state from `relay` and serve it, with the reload lock held.
    async fn install(&self, relay: Relay) -> caravel_util::error::Result<()> {
        let state = InstanceState::load(&self.core, relay).await?;
        self.pusher.retain(&state.relay.id, &state.push);
        *self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Arc::new(state);
        Ok(())
    }

    /// Reload the relay after a change to its access lists or rooms made outside
    /// it, then deliver what it published about the change.
    pub async fn apply_access_change(&self, published: &[Event]) {
        self.refresh().await;
        for event in published {
            self.broadcast(event);
        }
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
        caravel_relays::policy::screen(event)
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
                self.broadcast(&event);
                for event in &published {
                    self.broadcast(event);
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

        state.snapshot.admit_write(event, &authed, now)?;

        if !caravel_relays::events::is_storable(event) {
            return Err(Refusal::invalid("event contains a NUL character"));
        }

        if event.kind == kinds::PUSH_SUBSCRIPTION {
            caravel_relays::policy::check_push_subscription(
                event,
                &state.relay.hosts(),
                push::held_by(&state.push, event),
            )?;
            // Refused now for a clear answer; delivery refuses local addresses however named.
            let callback = event.tags.value("callback").unwrap_or_default();
            if self.core.outbound.check(callback).is_err() {
                return Err(Refusal::invalid(
                    "callback must not point to a local or internal address",
                ));
            }
        }

        let published = match event.kind {
            kinds::RELAY_JOIN => self.join(&state, event).await?,
            kinds::RELAY_LEAVE => self.leave(&state, event).await?,
            kind if !caravel_relays::policy::is_stored(kind) => Vec::new(),
            _ => self.store(&state, event).await?,
        };

        let changes_push = state.relay.push_enabled
            && matches!(event.kind, kinds::PUSH_SUBSCRIPTION | DELETION | VANISH);
        if !published.is_empty()
            || caravel_relays::rooms::is_moderation_kind(event.kind)
            || changes_push
        {
            self.refresh().await;
        }

        Ok(published)
    }

    /// Admit a NIP-43 join request: a member already, anyone on an open relay, or
    /// the holder of a live claim, within the plan's member limit. A paid invite's
    /// claim also gives the joiner its membership.
    async fn join(&self, state: &InstanceState, event: &Event) -> Result<Vec<Event>, Refusal> {
        let relay_id = state.relay.id.as_str();
        let join = state.snapshot.admit_join(event)?;
        let result = ingest::join(&self.core, &state.relay, &state.key, event, &join).await;

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
    async fn leave(&self, state: &InstanceState, event: &Event) -> Result<Vec<Event>, Refusal> {
        let relay = &state.relay;
        let pubkey = event.pubkey.to_hex();
        let result = ingest::leave(&self.core, relay, &state.key, &pubkey).await;

        match result {
            Ok(published) => Ok(published),
            Err(Error::Conflict { .. }) => Ok(Vec::new()),
            Err(error) => {
                tracing::warn!(relay = %relay.id, error = %error, "failed to remove a member");
                Err(Refusal::Error("the leave could not be applied"))
            }
        }
    }

    /// Keep an event, applying a deletion, a request to vanish, or a change to a
    /// room in the same transaction.
    async fn store(&self, state: &InstanceState, event: &Event) -> Result<Vec<Event>, Refusal> {
        if event.kind == VANISH && !self.is_addressed(&state.relay, event) {
            return Err(Refusal::invalid(
                "this request to vanish names another relay",
            ));
        }

        let relay_id = state.relay.id.as_str();
        let rooms_enabled = state.snapshot.groups_enabled;
        let (saved, published) =
            ingest::store(&self.core, &state.relay, &state.key, rooms_enabled, event)
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
        let hosts = relay.hosts();
        event.tags.find_all("relay").any(|tag| {
            tag.value() == "ALL_RELAYS"
                || hosts.contains(&caravel_util::urls::url_host(tag.value()))
        })
    }

    /// Deliver a new event, a client's or the relay's own, to the subscriptions it
    /// matches and, when push is on, to its push subscriptions.
    pub fn broadcast(&self, event: &Event) {
        self.fanout(event);
        let state = self.state();
        if state.relay.push_enabled {
            let now = self.core.clock.now();
            self.pusher.push(
                &state.snapshot,
                &state.push,
                &state.relay.host(),
                event,
                now,
            );
        }
    }

    /// Send an event to every subscription that matches it and whose connection
    /// may still read the relay and see the event, without signatures where the
    /// relay strips them.
    fn fanout(&self, event: &Event) {
        let matched = self.connections.matching(event);
        if matched.is_empty() {
            return;
        }

        let state = self.state();
        let now = self.core.clock.now();
        let facts = EventFacts::of(event);
        let stored = StoredEvent::from_event(event);
        let mut signed: Option<RawEvent> = None;
        let mut stripped: Option<RawEvent> = None;

        for matched in matched {
            if !state.snapshot.can_deliver(&matched.pubkeys, facts, now) {
                continue;
            }

            let strip = state.snapshot.strips_signature(&matched.pubkeys);
            let rendered = if strip { &mut stripped } else { &mut signed };
            let raw = rendered
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

        state.snapshot.admit_req(&authed)?;
        if filters.len() > self.limits.max_filters {
            return Err(Refusal::blocked(format!(
                "a request may have at most {} filters",
                self.limits.max_filters
            )));
        }

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

        let strip = state.snapshot.strips_signature(&authed);
        let mut sent = HashSet::new();

        for filter in &filters {
            let found = caravel_core::events::query_events(
                &self.core,
                &state.relay.id,
                filter,
                self.limits.max_limit,
                now,
                &authed,
            )
            .await
            .map_err(|error| {
                tracing::warn!(relay = %state.relay.id, error = %error, "query failed");
                self.connections.unsubscribe(session, subscription_id);
                Refusal::Error("the query could not be run")
            })?;

            for stored in found {
                if !sent.insert(stored.head.id)
                    || !state.snapshot.can_any_receive(
                        &authed,
                        EventFacts::of_head(&stored.head),
                        now,
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
            .send(RelayMessage::Eose(subscription_id.clone()))
            .await;
        Ok(())
    }

    pub fn on_close(&self, session: &Session, subscription_id: &SubscriptionId) {
        self.connections.unsubscribe(session, subscription_id);
    }

    pub async fn on_count(
        &self,
        session: &Session,
        subscription_id: SubscriptionId,
        filters: Vec<Filter>,
    ) {
        match self.count(session, &filters).await {
            Ok(count) => {
                session
                    .send(RelayMessage::Count {
                        subscription_id,
                        count,
                    })
                    .await;
            }
            Err(refusal) => {
                self.offer_challenge(session, &refusal).await;
                session
                    .send(refused_subscription(subscription_id, &refusal))
                    .await;
            }
        }
    }

    /// How many events the filters match that the connection may see: a count is
    /// admitted and filtered by the rules a subscription is.
    async fn count(&self, session: &Session, filters: &[Filter]) -> Result<u64, Refusal> {
        let state = self.state();
        let authed = self.connections.pubkeys(session);

        state.snapshot.admit_req(&authed)?;
        if filters.len() > self.limits.max_filters {
            return Err(Refusal::blocked(format!(
                "a request may have at most {} filters",
                self.limits.max_filters
            )));
        }

        let now = self.core.clock.now();

        let groups = caravel_core::events::count_event_groups(
            &self.core,
            &state.relay.id,
            filters,
            now,
            &authed,
        )
        .await
        .map_err(|error| {
            tracing::warn!(relay = %state.relay.id, error = %error, "count failed");
            Refusal::Error("the count could not be run")
        })?;

        Ok(state.snapshot.count_visible(&authed, &groups, now))
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
                .authenticate(session, pubkey, self.limits.max_identities)
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
        match caravel_core::events::get_notification(
            &self.core,
            &relay_id,
            &pubkey.to_hex(),
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
        let Ok(pubkey) = PublicKey::from_hex(pubkey) else {
            return;
        };
        for session in self.connections.sessions_for(&pubkey) {
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
        let now = self.core.clock.now();

        state.snapshot.admit_req(&authed)?;

        let heads = caravel_core::events::query_event_heads(
            &self.core,
            &state.relay.id,
            &filter,
            self.limits.max_sync,
            now,
            &authed,
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
                state
                    .snapshot
                    .can_any_receive(&authed, EventFacts::of_head(head), now)
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
