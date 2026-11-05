//! One exclusive mapping loan; the connection keeps servicing its offer ledger.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::*;

/// An owned, non-cloneable slot payload that may move to a render worker.
///
/// Return it to the same producer for commit or explicit abort. A successful
/// operation consumes its payload; a failed operation leaves custody here.
/// Dropping a live loan closes the producer, including pending offers becoming
/// Unknown. The caller must wake and poll the connection after worker completion
/// or abandonment, and bound the render job's lifetime itself. Neither a timeout
/// nor dropping the producer reclaims memory from a worker still holding this loan.
#[must_use = "return the loan for commit or abort; dropping it closes the ring"]
pub struct OwnedRingSlotFill {
    ring: Option<sophia_isb::MappedRing>,
    abandoned: Arc<AtomicBool>,
    slot: usize,
    offset: usize,
    byte_len: usize,
}

impl OwnedRingSlotFill {
    /// The exact logical frame payload, not the registration's full capacity.
    /// Refuses after successful commit or abort.
    pub fn pixels_mut(&mut self) -> Result<&mut [u8], ProducerError> {
        let ring = self.ring.as_mut().ok_or(ProducerError::Refused)?;
        Ok(&mut ring.bytes_mut()[self.offset..self.offset + self.byte_len])
    }
}

impl Drop for OwnedRingSlotFill {
    fn drop(&mut self) {
        if self.ring.is_some() {
            self.abandoned.store(true, Ordering::Release);
        }
    }
}

impl RingProducer {
    pub(super) fn owned_outstanding(&self) -> bool {
        self.owned.is_some() && self.ring.is_none()
    }

    pub(super) fn owned_abandoned(&self) -> bool {
        self.owned
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    }

    /// Acquire one worker-owned payload without spending an offer id or slot
    /// generation. Other offers can settle while this loan is outstanding, but
    /// all further uploads/acquisitions return Busy until it is returned.
    pub fn acquire_owned_slot(
        &mut self,
        byte_len: usize,
        now_ms: u64,
    ) -> Result<OwnedRingSlotFill, ProducerError> {
        self.clock(now_ms)?;
        if self.closed() {
            return Err(ProducerError::Closed);
        }
        if !self.ready() {
            return Err(ProducerError::Again);
        }
        if self.owned_outstanding() {
            return Err(ProducerError::Busy);
        }
        if byte_len == 0 || !byte_len.is_multiple_of(4) {
            return Err(ProducerError::Argument);
        }
        if byte_len as u64 > self.registration.capacity {
            return Err(ProducerError::Refused);
        }
        let slot = self.free_slot_index().ok_or(ProducerError::Busy)?;
        let ring = self.ring.take().ok_or(ProducerError::Closed)?;
        // Keep one producer identity across fills, without a per-frame allocation.
        let abandoned = Arc::clone(
            self.owned
                .get_or_insert_with(|| Arc::new(AtomicBool::new(false))),
        );
        self.slots[slot].acquired = true;
        Ok(OwnedRingSlotFill {
            ring: Some(ring),
            abandoned,
            slot,
            offset: slot * self.registration.slot_extent() as usize
                + OWNED_RING_HEADER_BYTES as usize,
            byte_len,
        })
    }

    fn owns_fill(&self, fill: &OwnedRingSlotFill) -> bool {
        fill.ring.is_some()
            && self
                .owned
                .as_ref()
                .is_some_and(|identity| Arc::ptr_eq(identity, &fill.abandoned))
    }

    /// Publish a returned payload. All errors preserve the loan for retry or
    /// abort; success leaves it inert. The caller must separately revalidate the
    /// current grant/allocation/output before supplying the upload identities.
    pub fn commit_owned_slot(
        &mut self,
        fill: &mut OwnedRingSlotFill,
        upload: RingUpload,
        now_ms: u64,
    ) -> Result<(u64, Vec<u8>), ProducerError> {
        self.commit_owned_offer(fill, upload, now_ms)
            .map(|offer| (offer.offer_id, offer.encode()))
    }

    /// Validate a returned loan and preview its exact offer without publishing it.
    /// Do not send the preview until [`Self::commit_owned_offer`] succeeds.
    /// # Errors
    /// Refuses foreign, consumed or closed loans and invalid upload geometry;
    /// the mapping and source identity remain with the caller.
    pub fn owned_offer(
        &mut self,
        fill: &OwnedRingSlotFill,
        upload: RingUpload,
        now_ms: u64,
    ) -> Result<RingOffer, ProducerError> {
        self.clock(now_ms)?;
        if self.closed() {
            return Err(ProducerError::Closed);
        }
        if !self.owns_fill(fill) {
            return Err(ProducerError::Refused);
        }
        if !self.ready() {
            return Err(ProducerError::Again);
        }
        self.check_offer(fill.slot, upload, fill.byte_len as u64)
            .map(|(_, offer)| offer)
    }

    /// Publish a returned loan as a typed offer without allocating encoded bytes.
    /// # Errors
    /// Uses [`Self::owned_offer`]'s checks; every error retains the loan for abort
    /// or retry. Success makes it inert and preserves normal source-release custody.
    pub fn commit_owned_offer(
        &mut self,
        fill: &mut OwnedRingSlotFill,
        upload: RingUpload,
        now_ms: u64,
    ) -> Result<RingOffer, ProducerError> {
        self.owned_offer(fill, upload, now_ms)?;
        self.ring = fill.ring.take();
        let result = self.commit_slot(fill.slot, upload, fill.byte_len as u64, now_ms);
        if result.is_err() {
            fill.ring = self.ring.take();
        }
        result
    }

    /// Return an unused payload, even after closure, without sending anything
    /// or spending an offer id/generation. This never reopens a closed producer.
    pub fn abort_owned_slot(&mut self, fill: &mut OwnedRingSlotFill) -> Result<(), ProducerError> {
        if !self.owns_fill(fill) {
            return Err(ProducerError::Refused);
        }
        self.ring = fill.ring.take();
        self.slots[fill.slot].acquired = false;
        Ok(())
    }
}
