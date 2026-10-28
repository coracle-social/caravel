//! Relay members: who belongs to a relay, how they joined, and the roles they
//! hold.

use serde::{Deserialize, Serialize};

/// How a pubkey became a member.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum MemberSource {
    /// The relay's tenant, who is always a member.
    Owner,
    /// Added by the tenant, an admin, or a pubkey granted the method.
    Manual,
    /// Joined with an invite claim.
    Claim,
    /// Joined a relay that takes anyone.
    PublicJoin,
    /// Paid for a membership.
    Payment,
    /// Carried over from zooid.
    Migrated,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct RelayMember {
    pub pubkey: String,
    pub source: MemberSource,
    /// The ids of the roles the member holds, in the order they were assigned.
    pub roles: Vec<String>,
    pub created_at: i64,
}

impl RelayMember {
    /// The tags of a relay's member list (kind 13534): every member and their roles.
    pub fn list_tags(members: &[RelayMember]) -> Vec<Vec<String>> {
        let mut tags = vec![crate::events::tag(&["-"])];
        tags.extend(members.iter().map(|member| {
            let mut tag = vec!["member".to_string(), member.pubkey.clone()];
            tag.extend(member.roles.iter().cloned());
            tag
        }));
        tags
    }
}
