//! Sessions: the bearer tokens a NIP-98 signature buys, held as their hashes.

/// How long a session lasts.
pub const SESSION_TTL: i64 = 30 * 24 * 60 * 60;

/// A sign-in, held as the hash of the bearer token that stands for it.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Session {
    pub pubkey: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub last_used_at: i64,
}
