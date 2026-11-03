//! Who gets events without signatures.

use coracle_lib::keys::PublicKey;

use crate::policy::RelaySnapshot;

impl RelaySnapshot {
    pub fn strips_signature(&self, authed: &[PublicKey]) -> bool {
        self.strip_signatures && !authed.iter().any(|reader| self.can_manage(reader))
    }
}
