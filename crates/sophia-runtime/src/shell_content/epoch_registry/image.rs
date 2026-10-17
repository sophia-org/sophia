//! Atomic resource/worker reservation for a decoded sealed ImageBegin.
use super::*;
use crate::{ContentAllocationError, SealedSubmitFailure, SealedValidationWorker};
use sophia_protocol::TransactionId;
use sophia_protocol::shell_files::ShellImageMetadata;
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SealedImageAdmissionError {
    Allocation(ContentAllocationError),
    Resource(ContentStoreError),
    Worker(SealedSubmitFailure),
}

/// Owner custody, not a journal event. The owner must publish ImageAdmitted
/// before allowing Source and reconcile this job even after losing the peer.
#[derive(Debug)]
pub struct SealedImageAdmission {
    pub metadata: ShellImageMetadata,
    pub worker_job: u64,
    pub grant: ContentGrant,
    pub deadline: Instant,
}

impl ContentEpochRegistry {
    pub(in crate::shell_content) fn sealed_stop_reason(
        &self,
        grant: ContentGrant,
        resource: sophia_protocol::ContentResourceId,
    ) -> Option<sophia_protocol::ContentReason> {
        self.active
            .iter()
            .chain(&self.retired)
            .find(|epoch| epoch.resources.grant() == grant)?
            .resources
            .sealed_stop_reason(resource)
    }

    pub(in crate::shell_content) fn stop_sealed_image(
        &mut self,
        grant: ContentGrant,
        resource: sophia_protocol::ContentResourceId,
        reason: sophia_protocol::ContentReason,
        now: u64,
    ) -> Result<sophia_protocol::ContentReason, ContentStoreError> {
        self.active
            .iter_mut()
            .chain(&mut self.retired)
            .find(|epoch| epoch.resources.grant() == grant)
            .ok_or(ContentStoreError::Stale)?
            .resources
            .stop_sealed(resource, reason, now)
    }

    /// Reserve the live resource and a worker credit without receiving an FD.
    /// The channel owner first validates its published limits, increasing offer
    /// ID and journal reservations. This method then checks the actual grant,
    /// output/allocation and topology; a supplied snapshot is not authority.
    /// `deadline` is fixed by the owner from the original admission time and
    /// the smaller of the transfer and validation timeouts.
    pub fn reserve_sealed_image(
        &mut self,
        grant: ContentGrant,
        metadata: ShellImageMetadata,
        worker: &mut SealedValidationWorker,
        now: u64,
        deadline: Instant,
    ) -> Result<SealedImageAdmission, SealedImageAdmissionError> {
        let epoch = self
            .active
            .iter_mut()
            .find(|epoch| epoch.resources.grant() == grant)
            .ok_or(SealedImageAdmissionError::Resource(
                ContentStoreError::Stale,
            ))?;
        let description = epoch
            .allocations
            .sealed_image_description(&metadata)
            .map_err(SealedImageAdmissionError::Allocation)?;
        let bytes = usize::try_from(description.total_bytes)
            .map_err(|_| SealedImageAdmissionError::Resource(ContentStoreError::Malformed))?;
        let job = worker
            .reserve(bytes, deadline)
            .map_err(SealedImageAdmissionError::Worker)?;
        if let Err(error) = epoch.resources.begin_sealed(
            TransactionId::from_raw(metadata.offer_id),
            description,
            now,
        ) {
            worker.release_reserved(job);
            return Err(SealedImageAdmissionError::Resource(error));
        }
        Ok(SealedImageAdmission {
            metadata,
            worker_job: job,
            grant,
            deadline,
        })
    }
}
