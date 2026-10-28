//! Hosting: the operations tenants and admins perform on tenants and relays, each
//! of which spans the relay row, its activity, billing and the relay's hosts.
//! Callers authorize before calling.

pub mod erasure;
pub mod relays;
pub mod tenants;

pub use erasure::*;
pub use relays::*;
pub use tenants::*;
