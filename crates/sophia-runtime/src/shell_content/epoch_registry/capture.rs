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
        epoch
            .resources
            .begin_capture(description, *metadata, now)
            .map_err(ContentCaptureAdmissionError::Resource)
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
