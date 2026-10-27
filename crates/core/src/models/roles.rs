//! Roles: labels a relay gives some of its members, which clients show.

use serde::{Deserialize, Serialize};

/// A label a relay gives some of its members, shown by clients.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct Role {
    pub id: String,
    pub label: String,
    pub description: String,
    /// A color hue from 0 to 360, or 0 for the client's default.
    pub hue: i32,
    /// Where the role sorts among the relay's roles, or 0 for the client's default.
    pub position: i32,
}
