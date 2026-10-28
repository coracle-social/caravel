//! Who gets events without signatures.

use crate::policy::RelaySnapshot;

impl RelaySnapshot {
    pub fn strips_signature(&self, authed: &[String]) -> bool {
        self.strip_signatures && !authed.iter().any(|reader| self.can_manage(reader))
    }
}
