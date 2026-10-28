//! Checking a role, and the event that defines it.

use crate as domain;
use crate::error::{Error, Result};
use crate::roles::Role;

impl Role {
    /// Check a role's id and hue.
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty() {
            return Err(Error::invalid("role-id-required", "role id is required"));
        }
        if !(0..=360).contains(&self.hue) {
            return Err(Error::invalid(
                "invalid-hue",
                "color hue must be between 0 and 360",
            ));
        }
        Ok(())
    }

    /// A role's definition as tags. A zero hue or position is left out, for the
    /// client to fall back on its own default.
    pub fn tags(&self) -> Vec<Vec<String>> {
        let mut tags = vec![
            domain::events::tag(&["-"]),
            domain::events::tag(&["d", &self.id]),
        ];
        if !self.label.is_empty() {
            tags.push(domain::events::tag(&["label", &self.label]));
        }
        if !self.description.is_empty() {
            tags.push(domain::events::tag(&["description", &self.description]));
        }
        if self.hue != 0 {
            tags.push(domain::events::tag(&["color", &self.hue.to_string()]));
        }
        if self.position != 0 {
            tags.push(domain::events::tag(&["order", &self.position.to_string()]));
        }
        tags
    }
}
