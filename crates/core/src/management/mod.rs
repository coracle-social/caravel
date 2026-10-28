//! Managing a relay: its members, NIP-86 calls, and room moderation from the
//! dashboards.

mod calls;
mod members;
mod moderation;
mod reads;
mod writes;

pub use calls::{call, run_call};
pub use members::{add_member, remove_member};
pub use moderation::moderate_room;
pub use reads::{access_lists, page_events, rooms};
