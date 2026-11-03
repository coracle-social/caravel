//! What a relay remembers about each open connection: the challenge it was
//! issued, the pubkeys it has proved, its open subscriptions, and its negentropy
//! reconciliations. All of it dies with the connection.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use coracle_lib::events::Event;
use coracle_lib::filters::Filter;
use coracle_lib::keys::PublicKey;
use coracle_lib::sync::SyncSet;

use crate::message::SubscriptionId;
use crate::session::Session;

#[derive(Debug, Default)]
pub struct Connections {
    inner: RwLock<HashMap<u64, Connection>>,
}

#[derive(Debug)]
struct Connection {
    session: Session,
    challenge: String,
    /// Every pubkey the connection has proved. A client holding several keys
    /// answers once for each.
    pubkeys: BTreeSet<PublicKey>,
    subscriptions: HashMap<SubscriptionId, Vec<Filter>>,
    /// Reconciliations in their own id namespace, each over a snapshot of the set
    /// taken when it opened.
    reconciliations: HashMap<SubscriptionId, Arc<SyncSet>>,
}

/// Why a subscription, reconciliation or identity wasn't registered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterError {
    /// The connection already holds as many as it may.
    TooMany,
    /// The connection closed meanwhile.
    Gone,
}

/// A live subscription an event matched, and who it reads as.
pub struct Match {
    pub session: Session,
    pub subscription_id: SubscriptionId,
    pub pubkeys: Vec<PublicKey>,
}

impl Connections {
    pub fn open(&self, session: &Session, challenge: String) {
        self.write().insert(
            session.id(),
            Connection {
                session: session.clone(),
                challenge,
                pubkeys: BTreeSet::new(),
                subscriptions: HashMap::new(),
                reconciliations: HashMap::new(),
            },
        );
    }

    pub fn close(&self, session: &Session) {
        self.write().remove(&session.id());
    }

    pub fn challenge(&self, session: &Session) -> Option<String> {
        self.read()
            .get(&session.id())
            .map(|connection| connection.challenge.clone())
    }

    /// The pubkeys a connection has proved, in order. Empty until it authenticates.
    pub fn pubkeys(&self, session: &Session) -> Vec<PublicKey> {
        self.read()
            .get(&session.id())
            .map(|connection| connection.pubkeys.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Record a pubkey the connection proved, up to `max` of them.
    pub fn authenticate(
        &self,
        session: &Session,
        pubkey: PublicKey,
        max: usize,
    ) -> Result<(), RegisterError> {
        let mut state = self.write();
        let connection = state.get_mut(&session.id()).ok_or(RegisterError::Gone)?;

        if !connection.pubkeys.contains(&pubkey) && connection.pubkeys.len() >= max {
            return Err(RegisterError::TooMany);
        }

        connection.pubkeys.insert(pubkey);
        Ok(())
    }

    /// Open a subscription, replacing one of the same id.
    pub fn subscribe(
        &self,
        session: &Session,
        subscription_id: SubscriptionId,
        filters: Vec<Filter>,
        max: usize,
    ) -> Result<(), RegisterError> {
        let mut state = self.write();
        let connection = state.get_mut(&session.id()).ok_or(RegisterError::Gone)?;

        // Replacing a subscription is how a client changes it, so only a new id counts.
        if !connection.subscriptions.contains_key(&subscription_id)
            && connection.subscriptions.len() >= max
        {
            return Err(RegisterError::TooMany);
        }

        connection.subscriptions.insert(subscription_id, filters);
        Ok(())
    }

    pub fn unsubscribe(&self, session: &Session, subscription_id: &SubscriptionId) {
        if let Some(connection) = self.write().get_mut(&session.id()) {
            connection.subscriptions.remove(subscription_id);
        }
    }

    /// Open a reconciliation, replacing one of the same id.
    pub fn open_reconciliation(
        &self,
        session: &Session,
        subscription_id: SubscriptionId,
        set: Arc<SyncSet>,
        max: usize,
    ) -> Result<(), RegisterError> {
        let mut state = self.write();
        let connection = state.get_mut(&session.id()).ok_or(RegisterError::Gone)?;

        if !connection.reconciliations.contains_key(&subscription_id)
            && connection.reconciliations.len() >= max
        {
            return Err(RegisterError::TooMany);
        }

        connection.reconciliations.insert(subscription_id, set);
        Ok(())
    }

    pub fn reconciliation(
        &self,
        session: &Session,
        subscription_id: &SubscriptionId,
    ) -> Option<Arc<SyncSet>> {
        self.read()
            .get(&session.id())?
            .reconciliations
            .get(subscription_id)
            .cloned()
    }

    pub fn close_reconciliation(&self, session: &Session, subscription_id: &SubscriptionId) {
        if let Some(connection) = self.write().get_mut(&session.id()) {
            connection.reconciliations.remove(subscription_id);
        }
    }

    /// Every subscription a filter of which matches the event. Matches are
    /// collected under the lock and delivered outside it.
    pub fn matching(&self, event: &Event) -> Vec<Match> {
        let state = self.read();
        let mut matched = Vec::new();

        for connection in state.values() {
            for (subscription_id, filters) in &connection.subscriptions {
                if filters.iter().any(|filter| filter.matches(event)) {
                    matched.push(Match {
                        session: connection.session.clone(),
                        subscription_id: subscription_id.clone(),
                        pubkeys: connection.pubkeys.iter().cloned().collect(),
                    });
                }
            }
        }

        matched
    }

    /// Every connection authenticated as a pubkey.
    pub fn sessions_for(&self, pubkey: &PublicKey) -> Vec<Session> {
        self.read()
            .values()
            .filter(|connection| connection.pubkeys.contains(pubkey))
            .map(|connection| connection.session.clone())
            .collect()
    }

    fn read(&self) -> RwLockReadGuard<'_, HashMap<u64, Connection>> {
        self.inner.read().unwrap_or_else(|error| error.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, HashMap<u64, Connection>> {
        self.inner
            .write()
            .unwrap_or_else(|error| error.into_inner())
    }
}
