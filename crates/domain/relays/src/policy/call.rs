//! Who may join a room's call.

use coracle_lib::keys::PublicKey;

use crate::policy::{CallRefusal, RelaySnapshot};

impl RelaySnapshot {
    pub fn admit_call(&self, pubkey: &PublicKey, room_id: &str) -> Result<(), CallRefusal> {
        if !self.has_access(pubkey) {
            return Err(CallRefusal::NotMember);
        }
        let room = self.rooms.get(room_id).ok_or(CallRefusal::NoRoom)?;
        if !room.livekit {
            return Err(CallRefusal::NoCalls);
        }
        if (room.private || room.restricted) && !self.has_room_access(room, pubkey) {
            return Err(CallRefusal::NotRoomMember);
        }
        Ok(())
    }
}
