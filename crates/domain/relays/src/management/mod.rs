//! NIP-86 calls: decoding them and what carrying one out did.

mod decode;
mod methods;
mod model;

pub mod commands;
pub mod queries;

pub use methods::*;
pub use model::*;
