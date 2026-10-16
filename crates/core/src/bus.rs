//! In-process notifications of committed changes. Writers publish after their
//! transaction commits, so a subscriber only ever sees durable state; a message is
//! a hint to re-read, and a subscriber that lags re-reads everything it watches.

use coracle_lib::events::Event;
use tokio::sync::broadcast;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// A relay's row, hosts or status changed.
    RelayChanged { relay_id: String },
    /// A relay's members, roles, grants, bans or rooms changed, and it published
    /// these events about it.
    AccessChanged {
        relay_id: String,
        published: Vec<Event>,
    },
    /// A pubkey's notification on a relay was replaced with this message.
    Notified {
        relay_id: String,
        pubkey: String,
        message: String,
    },
}

#[derive(Clone, Debug)]
pub struct Bus {
    sender: broadcast::Sender<Change>,
}

impl Default for Bus {
    fn default() -> Self {
        Self {
            sender: broadcast::channel(1024).0,
        }
    }
}

impl Bus {
    /// Tell subscribers about a committed change. Nobody listening is fine.
    pub fn publish(&self, change: Change) {
        let _ = self.sender.send(change);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.sender.subscribe()
    }
}
