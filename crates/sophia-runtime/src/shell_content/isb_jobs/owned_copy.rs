//! Distinct mutable-ring ingress, sharing the existing CPU job lifecycle.
use super::*;
use crate::shell_content::owned_ring::OwnedRingBinding;
use crate::{OwnedRing, OwnedRingRead, OwnedSlotIdentity};
use sophia_protocol::{ContentAllocationId, ContentOutputId};

/// Internal experiment request, not an allocated public source kind or codec.
/// Geometry describes the owned destination; the ring fixes all source bounds.
#[derive(Clone, Copy, Debug)]
pub struct OwnedCopyImageRequest {
    pub slot: OwnedSlotIdentity,
    pub resource: ContentResourceId,
    pub allocation: ContentAllocationId,
    pub output: ContentOutputId,
    pub topology_generation: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub struct OwnedCopyImageMetadata {
    pub(crate) request: OwnedCopyImageRequest,
    pub(crate) binding: OwnedRingBinding,
    pub(crate) description: sophia_protocol::ContentResourceBegin,
}

impl OwnedCopyImageMetadata {
    pub fn request(&self) -> &OwnedCopyImageRequest {
        &self.request
    }
}

impl metadata_sealed::Sealed for OwnedCopyImageMetadata {}
impl CpuImageMetadata for OwnedCopyImageMetadata {
    fn resource(&self) -> ContentResourceId {
        self.request.resource
    }
    fn allocation_current(&self, registry: &ContentEpochRegistry, grant: ContentGrant) -> bool {
        registry
            .owned_copy_description(grant, &self.request)
            .as_ref()
            == Some(&self.description)
    }
}

pub type OwnedCopyImageJobs = CpuImageJobs<OwnedCopyImageMetadata>;
pub type OwnedCopyImageAdmission = CpuImageAdmission<OwnedCopyImageMetadata>;
pub type OwnedCopyImageSettlement = CpuImageSettlement<OwnedCopyImageMetadata>;

impl OwnedCopyImageJobs {
    /// Reserve destination and worker credits against the ring's actual grant.
    /// The export owner must first hold terminal/slot-release journal credits
    /// and check current custody. No source read or slot handoff occurs here.
    pub fn reserve(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker,
        ring: &OwnedRing,
        request: OwnedCopyImageRequest,
        now: u64,
        deadline: Instant,
    ) -> Result<u64, IsbImageAdmissionError> {
        let slot = self
            .pending
            .iter_mut()
            .find(|s| s.is_none())
            .ok_or(IsbImageAdmissionError::Worker(IsbSubmitFailure::Capacity))?;
        let admission = registry.reserve_owned_copy_image(ring, request, worker, now, deadline)?;
        let job = admission.worker_job;
        *slot = Some(Pending {
            admission,
            result: None,
            failure: None,
            submitted: false,
            stopped: None,
        });
        Ok(job)
    }

    /// Only the exact reserved ring/offer/slot/generation may supply this job.
    /// A refusal returns its ticket intact: it still owes its own slot release.
    /// Keep the table paired with the same worker for its entire lifetime.
    pub fn submit(
        &mut self,
        worker: &mut IsbValidationWorker,
        job: u64,
        source: OwnedRingRead,
    ) -> Result<(), (IsbSubmitFailure, OwnedRingRead)> {
        let Some(pending) = self.pending.iter_mut().flatten().find(|pending| {
            pending.admission.worker_job == job
                && !pending.submitted
                && pending.stopped.is_none()
                && pending.result.is_none()
                && source.matches(
                    &pending.admission.metadata.binding,
                    pending.admission.metadata.request.slot,
                )
        }) else {
            return Err((IsbSubmitFailure::Invalid, source));
        };
        worker.submit_ring_reserved(job, source)?;
        pending.submitted = true;
        Ok(())
    }
}
