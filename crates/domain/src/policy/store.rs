//! Whether the relay keeps an event or only passes it on.

use coracle_lib::kinds::is_ephemeral;

/// Whether the relay keeps an event or only passes it on.
pub fn is_stored(kind: u16) -> bool {
    !is_ephemeral(kind)
}
