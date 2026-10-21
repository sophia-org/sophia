//! Capture reservations belong to the actual allocation and resource owner.
use super::*;
use crate::{ContentAllocationError, ContentCaptureFailure, ContentCaptureReservation};
use sophia_protocol::{ContentReason, shell_files::ShellImageMetadata};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentCaptureAdmissionError {
    Allocation(ContentAllocationError),
    Resource(ContentStoreError),
}

impl ContentEpochRegistry {
    /// Current allocation target for an exact reserved capture. This checks
    /// logical ownership, not source completion or renderer capability.
    pub fn capture_output(
        &mut self,
        held: &ContentCaptureReservation,
        now: u64,
    ) -> Result<sophia_protocol::ContentOutputId, ContentStoreError> {
        let epoch = self
            .active
            .iter_mut()
            .find(|e| e.resources.grant() == held.description().grant)
            .ok_or(ContentStoreError::Stale)?;
        if epoch.resources.capture_stop_reason(held, now)?.is_some() {
            return Err(ContentStoreError::Revoked);
        }
        let current = epoch
            .allocations
            .capture_image_description(held.metadata())
            .map_err(|_| ContentStoreError::Stale)?;
        let allocation = epoch
            .allocations
            .capture_image_allocation(held.metadata())
            .map_err(|_| ContentStoreError::Stale)?;
        if &current != held.description() || held.allocation() != Some(&allocation) {
            return Err(ContentStoreError::Stale);
        }
        Ok(allocation.output)
    }

    /// Trusted image-owner accounting after an exact native completion receipt.
    /// A retained snapshot is NOT that proof. Session's private settlement path
    /// supplies it only from the opaque receipt, never from a peer or caller's
    /// claimed completion. Refusals preserve both ownership tickets.
    pub fn accept_capture_image(
        &mut self,
        reservation: &mut Option<ContentCaptureReservation>,
        image: &mut Option<std::sync::Arc<crate::ContentCapturedImage>>,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        let held = reservation.as_ref().ok_or(ContentStoreError::Stale)?;
        let epoch = self
            .active
            .iter_mut()
            .find(|e| e.resources.grant() == held.description().grant)
            .ok_or(ContentStoreError::Stale)?;
        // The allocation, topology and grant may have changed while GPU work ran.
        let current = epoch
            .allocations
            .capture_image_description(held.metadata())
            .map_err(|_| ContentStoreError::Stale)?;
        if &current != held.description() {
            return Err(ContentStoreError::Stale);
        }
        let allocation = epoch
            .allocations
            .capture_image_allocation(held.metadata())
            .map_err(|_| ContentStoreError::Stale)?;
        if held.allocation() != Some(&allocation) {
            return Err(ContentStoreError::Stale);
        }
        epoch.resources.accept_capture(reservation, image, now)
    }

    /// Visit active and disconnected resource owners with one bounded native
    /// eviction budget. Disconnect must not strand cached imports in a retired
    /// epoch. Actual frame/storage consumers remain charged after eviction.
    pub fn retire_capture_imports<E>(
        &mut self,
        remaining: &mut usize,
        evict: &mut impl FnMut(&std::sync::Arc<crate::ContentCapturedImage>) -> Result<(), E>,
    ) -> Result<(), E> {
        for epoch in self.active.iter_mut().chain(&mut self.retired) {
            epoch.resources.retire_capture_imports(remaining, evict)?;
        }
        self.collect();
        Ok(())
    }

    pub fn capture_retirement_pending(&self) -> bool {
        self.active
            .iter()
            .chain(&self.retired)
            .any(|epoch| epoch.resources.capture_retirement_pending())
    }

    pub fn capture_retirement_active(&self) -> bool {
        self.active
            .iter()
            .chain(&self.retired)
            .any(|epoch| epoch.resources.capture_retirement_active())
    }

    /// Reserve logical resource/journal-terminal credit for an opaque captured
    /// image. Does not enable DMA-BUF, publish ImageAdmitted or accept a source.
    /// The channel owner still owes negotiated-format/limits and offer checks.
    pub fn reserve_capture_image(
        &mut self,
        grant: ContentGrant,
        metadata: &ShellImageMetadata,
        now: u64,
    ) -> Result<ContentCaptureReservation, ContentCaptureAdmissionError> {
        let epoch = self
            .active
            .iter_mut()
            .find(|e| e.resources.grant() == grant)
            .ok_or(ContentCaptureAdmissionError::Resource(
                ContentStoreError::Stale,
            ))?;
        let description = epoch
            .allocations
            .capture_image_description(metadata)
            .map_err(ContentCaptureAdmissionError::Allocation)?;
        let allocation = epoch
            .allocations
            .capture_image_allocation(metadata)
            .map_err(ContentCaptureAdmissionError::Allocation)?;
        let mut reservation = epoch
            .resources
            .begin_capture(description, *metadata, now)
            .map_err(ContentCaptureAdmissionError::Resource)?;
        reservation.bind_allocation(allocation);
        Ok(reservation)
    }

    pub fn capture_stop_reason(
        &mut self,
        reservation: &ContentCaptureReservation,
        now: u64,
    ) -> Result<Option<ContentReason>, ContentStoreError> {
        self.active
            .iter_mut()
            .chain(&mut self.retired)
            .find(|e| e.resources.grant() == reservation.description().grant)
            .ok_or(ContentStoreError::Stale)?
            .resources
            .capture_stop_reason(reservation, now)
    }

    /// May reconcile a disconnected epoch, without reopening its authority.
    /// This accounting operation is not a source-read completion proof.
    pub fn fail_capture(
        &mut self,
        reservation: &mut Option<ContentCaptureReservation>,
        failure: ContentCaptureFailure,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        let grant = reservation
            .as_ref()
            .ok_or(ContentStoreError::Stale)?
            .description()
            .grant;
        let epoch = self
            .active
            .iter_mut()
            .chain(&mut self.retired)
            .find(|e| e.resources.grant() == grant)
            .ok_or(ContentStoreError::Stale)?;
        epoch.resources.fail_capture(reservation, failure, now)?;
        self.collect();
        Ok(())
    }
}
