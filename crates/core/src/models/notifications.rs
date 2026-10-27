//! Notifications: one message per pubkey per relay, which the relay sends as a
//! `NOTIFY` to every connection authenticated as that pubkey, now and whenever it
//! authenticates again, until the message is replaced, cleared or expires.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Notification {
    pub pubkey: String,
    pub message: String,
    pub created_at: i64,
    /// When the message stops being sent, if ever.
    pub expires_at: Option<i64>,
}
