//! Checking a role, and the event that defines it.

use crate::domain;
use crate::error::{Error, Result};
use crate::models::roles::Role;

pub fn validate_role(role: &Role) -> Result<()> {
    if role.id.is_empty() {
        return Err(Error::invalid("role-id-required", "role id is required"));
    }
    if !(0..=360).contains(&role.hue) {
        return Err(Error::invalid(
            "invalid-hue",
            "color hue must be between 0 and 360",
        ));
    }
    Ok(())
}

/// A role's definition as tags. A zero hue or position is left out, for the client
/// to fall back on its own default.
pub fn role_tags(role: &Role) -> Vec<Vec<String>> {
    let mut tags = vec![
        domain::events::tag(&["-"]),
        domain::events::tag(&["d", &role.id]),
    ];
    if !role.label.is_empty() {
        tags.push(domain::events::tag(&["label", &role.label]));
    }
    if !role.description.is_empty() {
        tags.push(domain::events::tag(&["description", &role.description]));
    }
    if role.hue != 0 {
        tags.push(domain::events::tag(&["color", &role.hue.to_string()]));
    }
    if role.position != 0 {
        tags.push(domain::events::tag(&["order", &role.position.to_string()]));
    }
    tags
}
