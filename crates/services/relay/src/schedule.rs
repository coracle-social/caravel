//! The scheduled events coming due within the hour, held in memory so the relays
//! can deliver each one at its time. The database is read for the next hour's
//! worth as each hour runs out.

use std::collections::BTreeSet;
use std::sync::Mutex;

use caravel_core::Core;
use caravel_relays::events::EventHead;
use caravel_util::error::Result;

pub struct Schedule {
    queue: Mutex<Queue>,
}

struct Queue {
    /// Each event held, as its time, its relay's id and its id.
    held: BTreeSet<(i64, String, [u8; 32])>,
    /// The end of the window loaded, past which an event waits for a later load.
    until: i64,
}

impl Schedule {
    /// A schedule holding nothing, whose first window begins at `now`.
    pub fn new(now: i64) -> Self {
        Self {
            queue: Mutex::new(Queue {
                held: BTreeSet::new(),
                until: now,
            }),
        }
    }

    /// Hold a stored event until its time, if that falls in the window loaded.
    pub fn add(&self, relay_id: &str, head: &EventHead) {
        let mut queue = self.lock();
        if head.created_at <= queue.until {
            queue
                .held
                .insert((head.created_at, relay_id.to_string(), head.id));
        }
    }

    /// Load the hour ahead once the window has run out. The window moves before the
    /// read, so an event stored meanwhile is either read or added.
    pub async fn refill(&self, core: &Core, now: i64) -> Result<()> {
        let after = {
            let mut queue = self.lock();
            if now < queue.until {
                return Ok(());
            }
            std::mem::replace(&mut queue.until, now + 3600)
        };

        match caravel_core::events::list_scheduled_events(core, after, now + 3600).await {
            Ok(scheduled) => {
                let mut queue = self.lock();
                for (relay_id, head) in scheduled {
                    queue.held.insert((head.created_at, relay_id, head.id));
                }
                Ok(())
            }
            Err(error) => {
                self.lock().until = after;
                Err(error)
            }
        }
    }

    /// Take the events whose time has come, as their relays' ids and their ids,
    /// oldest first.
    pub fn take_due(&self, now: i64) -> Vec<(String, [u8; 32])> {
        let mut queue = self.lock();
        let later = queue.held.split_off(&(now + 1, String::new(), [0; 32]));
        std::mem::replace(&mut queue.held, later)
            .into_iter()
            .map(|(_, relay_id, id)| (relay_id, id))
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|error| error.into_inner())
    }
}
