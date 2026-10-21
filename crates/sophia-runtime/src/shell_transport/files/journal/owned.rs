//! Experimental records share the normal journal, sequence and ACK custody.
//! Their caller must first establish explicit experimental-file admission.
use super::*;
use crate::owned_ring_protocol::Event;

impl Journal {
    pub(super) fn owned_event_bytes(&self, mut event: Event) -> Result<Vec<u8>, Errno> {
        if event.identity.grant.connection_epoch != self.custody.epoch() {
            return Err(Errno::EINVAL);
        }
        event.sequence = self.custody.next_sequence();
        let bytes = event.encode();
        // A typed constructor is not authority to bypass canonical validation.
        if Event::decode(&bytes) != Some(event) {
            return Err(Errno::EINVAL);
        }
        Ok(bytes)
    }

    /// Only this exact epoch-scoped reservation is spent. Failures retain it.
    pub(in crate::shell_transport) fn append_owned_event(
        &mut self,
        reservation: JournalReservation,
        event: Event,
    ) -> Result<u64, Errno> {
        let slot = self
            .held
            .iter()
            .position(|slot| slot.is_some_and(|(held, _)| held == reservation))
            .ok_or(Errno::EINVAL)?;
        let held = self.held[slot].expect("reservation found").1;
        let bytes = self.owned_event_bytes(event)?;
        if bytes.len() != held {
            return Err(Errno::EINVAL);
        }
        let mut limits = self.append_limits(true);
        limits.records += 1;
        limits.bytes += held;
        let prepared = self.custody.prepare(bytes, limits)?;
        self.held[slot] = None;
        Ok(prepared.commit())
    }
}
