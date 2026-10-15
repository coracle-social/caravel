//! Clients for the systems outside caravel's database. Nothing here knows about
//! tenants, relays or billing: each adapter speaks one protocol and hands back
//! plain values for the core to interpret.

pub mod bitcoin;
pub mod dns;
pub mod robot;
pub mod stripe;
pub mod wallet;
pub mod zooid;
