//! The backend's source of the current time. Every timestamp it writes goes
//! through [`now`], which tests pin.

use std::sync::atomic::{AtomicI64, Ordering};

/// A pinned time for [`now`] to report, or `0` for the system clock. Only tests
/// pin it; production leaves it at zero for the life of the process.
static PINNED: AtomicI64 = AtomicI64::new(0);

/// The current unix timestamp, in seconds.
pub fn now() -> i64 {
    match PINNED.load(Ordering::SeqCst) {
        0 => chrono::Utc::now().timestamp(),
        at => at,
    }
}

// Only the integration tests pin the clock, reaching it through the lib crate;
// main.rs declares its own private copy of every module, where these look dead.
// `allow` rather than `expect`, since the lib build doesn't fire the lint.

/// Pin [`now`] to `at` until it is pinned again or released.
#[allow(dead_code)]
pub fn pin(at: i64) {
    PINNED.store(at, Ordering::SeqCst);
}

/// Hand [`now`] back to the system clock.
#[allow(dead_code)]
pub fn release() {
    PINNED.store(0, Ordering::SeqCst);
}
