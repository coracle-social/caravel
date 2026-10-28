//! Who a request runs as.

use serde::Serialize;

use crate::config::Config;
use crate::error::{Error, Result};

/// The pubkey a request runs as, and whether it administers the platform.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Actor {
    pub pubkey: String,
    pub is_admin: bool,
}

impl Actor {
    pub fn new(config: &Config, pubkey: &str) -> Self {
        Self {
            pubkey: pubkey.to_string(),
            is_admin: config.is_admin(pubkey),
        }
    }

    pub fn require_admin(&self) -> Result<()> {
        if self.is_admin {
            Ok(())
        } else {
            Err(Error::Forbidden("admin required"))
        }
    }

    /// Allow the actor to act as `pubkey`: being it, or administering the platform.
    pub fn require_pubkey_or_admin(&self, pubkey: &str) -> Result<()> {
        if self.is_admin || self.pubkey == pubkey {
            Ok(())
        } else {
            Err(Error::Forbidden("not authorized"))
        }
    }

    pub fn require_pubkey(&self, pubkey: &str) -> Result<()> {
        if self.pubkey == pubkey {
            Ok(())
        } else {
            Err(Error::Forbidden("not authorized"))
        }
    }
}
