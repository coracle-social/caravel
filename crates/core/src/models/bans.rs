//! Bans: the pubkeys and events a relay refuses.

use serde::{Deserialize, Serialize};

/// A banned pubkey or event id, and why.
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Ban {
    pub subject: String,
    pub reason: String,
    pub created_at: i64,
}
