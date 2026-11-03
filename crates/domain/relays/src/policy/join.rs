//! How a NIP-43 join request is admitted.

use coracle_lib::events::Event;

use crate::policy::{Join, Refusal, RelaySnapshot};

impl RelaySnapshot {
    pub fn admit_join(&self, event: &Event) -> Result<Join, Refusal> {
        let author = event.pubkey;

        if self.members.contains(&author) {
            return Ok(Join::AlreadyMember);
        }

        if self.is_banned(&author) {
            return Err(Refusal::blocked("you have been banned from this relay"));
        }

        if self
            .member_limit
            .is_some_and(|limit| self.members.len() >= limit)
        {
            return Err(Refusal::restricted(
                "this relay has reached its member limit",
            ));
        }

        if self.public_join {
            return Ok(Join::Open);
        }

        match event.tags.value("claim") {
            Some(claim) => Ok(Join::Claim(claim.to_string())),
            None if self.charges_members => {
                Err(Refusal::restricted("this space requires a paid membership"))
            }
            None => Err(Refusal::restricted("no claim tag")),
        }
    }
}
