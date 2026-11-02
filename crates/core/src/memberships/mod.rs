//! Paid memberships: members paying a relay's tenant, through Stripe Connect, for
//! access to the relay. A membership drives the relay's access list in the
//! transaction that changes it. Paying admits its member, or registers its invite
//! as a claim for whoever joins with it; lapsing or being removed from the relay
//! ends the membership and stops what it pays through, and lapsing tells the member
//! why.

pub mod checkout;
pub mod reads;
pub mod sweep;

pub use checkout::*;
pub use reads::*;
pub use sweep::*;
