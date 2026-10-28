//! Moving a relay's events in and out as JSONL, one event per line: what an import
//! reports, what one line of a dump holds, and what it keeps of a room's history.

use coracle_lib::events::Event;
use serde::{Deserialize, Serialize};

/// How many problems an import reports one by one; past this they are only counted.
pub const MAX_PROBLEMS: usize = 100;

/// Something in an import that didn't become a stored event: a line, the event on
/// it, or a room.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportProblem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    pub message: String,
}

/// An event of a room's history an import stored, kept small until the room is
/// rebuilt from it: enough to order it, find its kind, and read it back by id.
#[derive(Clone, Copy, Debug)]
pub struct HistoryEntry {
    pub created_at: i64,
    pub kind: u16,
    pub id: [u8; 32],
}

/// What an import did. An event the relay already held counts as imported, so
/// importing a dump twice reports the same numbers. Skipped counts the events
/// describing the source relay's own state, which this relay writes itself.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ImportResult {
    pub imported: usize,
    pub invalid: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub skipped: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rooms_rebuilt: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<ImportProblem>,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

impl ImportResult {
    pub fn fail(&mut self, problem: ImportProblem) {
        self.invalid += 1;
        self.note(problem);
    }

    pub fn note(&mut self, problem: ImportProblem) {
        if self.problems.len() < MAX_PROBLEMS {
            self.problems.push(problem);
        }
    }

    /// Count what another part of the import did.
    pub fn add(&mut self, other: ImportResult) {
        self.imported += other.imported;
        self.invalid += other.invalid;
        self.skipped += other.skipped;
        self.rooms_rebuilt += other.rooms_rebuilt;
        for problem in other.problems {
            self.note(problem);
        }
    }
}

/// What one line of a dump holds.
pub enum Line {
    Blank,
    Invalid(ImportProblem),
    /// An event describing the source relay's own state.
    SourceState,
    Event(Event),
}
