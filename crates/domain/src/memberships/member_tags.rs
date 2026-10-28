//! The events a relay publishes about its members.

use crate as domain;
use crate::memberships::RelayMember;

impl RelayMember {
    /// The tags of a relay's member list (kind 13534): every member and their roles.
    pub fn list_tags(members: &[RelayMember]) -> Vec<Vec<String>> {
        let mut tags = vec![domain::events::tag(&["-"])];
        tags.extend(members.iter().map(|member| {
            let mut tag = vec!["member".to_string(), member.pubkey.clone()];
            tag.extend(member.roles.iter().cloned());
            tag
        }));
        tags
    }
}
