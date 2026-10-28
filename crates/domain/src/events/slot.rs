//! What the event store reads off an event before keeping it.

use coracle_lib::addresses::get_identifier;
use coracle_lib::events::Event;
use coracle_lib::kinds::{is_addressable, is_replaceable};

/// The slot a replaceable or addressable event occupies: `""` for replaceable
/// kinds and the `d` tag for addressable ones. Regular events have none.
pub fn slot(event: &Event) -> Option<String> {
    if is_replaceable(event.kind) {
        Some(String::new())
    } else if is_addressable(event.kind) {
        Some(get_identifier(&event.tags).to_string())
    } else {
        None
    }
}

/// Whether Postgres text can hold the event: no NUL characters.
pub fn is_storable(event: &Event) -> bool {
    !event.content.contains('\0')
        && !event
            .tags
            .iter()
            .any(|tag| tag.as_slice().iter().any(|value| value.contains('\0')))
}
