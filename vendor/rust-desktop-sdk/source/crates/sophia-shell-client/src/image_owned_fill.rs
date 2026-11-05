use std::time::Instant;

use crate::{ImageAdapterError, OwnedRingSlotFill, RingUpload, ShellConnection};

impl ShellConnection {
    /// Acquire an exact-length payload for a render worker. Continue polling
    /// this connection on its owner thread while the worker holds the loan.
    /// The caller must bound the job and wake the owner on completion or panic.
    /// A second acquisition or upload is busy until the loan is returned.
    pub fn image_acquire_ring_fill(
        &mut self,
        byte_len: usize,
        now: Instant,
    ) -> Result<OwnedRingSlotFill, ImageAdapterError> {
        self.image
            .as_mut()
            .ok_or(ImageAdapterError::NotEnabled)?
            .acquire_owned_fill(&self.wire, byte_len, now)
    }

    /// Commit the worker's returned payload after revalidating the current
    /// grant, allocation, output and rendering identity. Any error retains the
    /// loan, so Again can be retried without rerendering. On success the loan is
    /// inert and the returned offer follows normal status/SlotReleased custody.
    /// This legacy offer is refused on r13; use [`crate::FrameStream::submit_ring_fill`]
    /// to join the returned loan to complete frame intent and credit instead, or
    /// [`crate::FrameStream::commit_legacy_ring_fill`] after draining for fallback.
    pub fn image_commit_ring_fill(
        &mut self,
        fill: &mut OwnedRingSlotFill,
        upload: RingUpload,
        now: Instant,
    ) -> Result<u64, ImageAdapterError> {
        let offer = self
            .image
            .as_mut()
            .ok_or(ImageAdapterError::NotEnabled)?
            .commit_owned_fill(&self.wire, fill, upload, now)?;
        let _ = self.poll_io();
        Ok(offer)
    }

    /// Return an unused loan without publication, including after disconnection.
    /// Unlike dropping a live loan, explicit abort does not fault an open ring.
    pub fn image_abort_ring_fill(
        &mut self,
        fill: &mut OwnedRingSlotFill,
    ) -> Result<(), ImageAdapterError> {
        self.image
            .as_mut()
            .ok_or(ImageAdapterError::NotEnabled)?
            .abort_owned_fill(fill)
    }
}
