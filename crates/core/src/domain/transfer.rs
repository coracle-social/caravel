//! Reading a line of a JSONL dump.

use coracle_lib::events::Event;

use crate::domain;
use crate::models::transfer::{ImportProblem, Line};

pub fn parse_line(line: usize, text: &[u8]) -> Line {
    let text = String::from_utf8_lossy(text);
    let text = text.trim();
    if text.is_empty() {
        return Line::Blank;
    }

    let event: Event = match serde_json::from_str(text) {
        Ok(event) => event,
        Err(error) => {
            return Line::Invalid(ImportProblem {
                line: Some(line),
                event_id: None,
                message: format!("json parse error: {error}"),
            });
        }
    };

    if event.verify().is_err() {
        return Line::Invalid(ImportProblem {
            line: Some(line),
            event_id: Some(event.id.to_hex()),
            message: "invalid signature".to_string(),
        });
    }

    if is_source_state(event.kind) {
        return Line::SourceState;
    }

    Line::Event(event)
}

/// Events the source relay published about its own members, roles, media and
/// rooms, which this relay derives from its own tables instead.
pub fn is_source_state(kind: u16) -> bool {
    domain::policy::is_relay_written(kind) || domain::rooms::is_metadata_kind(kind)
}
