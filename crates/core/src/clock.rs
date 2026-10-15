//! The only source of the current time, which tests pin.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

/// Reads unix seconds from the system, or from a pinned value while one is set.
#[derive(Clone, Debug, Default)]
pub struct Clock {
    pinned: Arc<AtomicI64>,
}

impl Clock {
    /// The current unix timestamp, in seconds.
    pub fn now(&self) -> i64 {
        match self.pinned.load(Ordering::SeqCst) {
            #[allow(clippy::disallowed_methods)]
            0 => chrono::Utc::now().timestamp(),
            at => at,
        }
    }

    /// Report `at` until pinned again or released.
    pub fn pin(&self, at: i64) {
        self.pinned.store(at, Ordering::SeqCst);
    }

    /// Go back to the system clock.
    pub fn release(&self) {
        self.pinned.store(0, Ordering::SeqCst);
    }
}
