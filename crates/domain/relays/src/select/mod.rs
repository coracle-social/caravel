//! SELECT: JMESPath expressions over a relay's events, which group the events into
//! rows and aggregate each row, with salted samples that let clients merge the
//! answers of several relays.

mod bolt11;
mod evaluate;
mod expression;
mod model;
mod sample;

pub use evaluate::{Aggregation, Plan, TooManyGroups};
pub use expression::Expression;
pub use model::*;
pub use sample::rank;
