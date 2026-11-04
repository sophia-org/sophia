//! Distinct mutable-ring ingress, sharing the existing CPU job lifecycle.
use super::*;
use crate::shell_content::owned_ring::OwnedRingBinding;
use crate::{OwnedRing, OwnedRingRead, OwnedSlotIdentity};
use sophia_protocol::{ContentAllocationId, ContentOutputId};

/// Internal experiment request, not an allocated public source kind or codec.
/// Geometry describes the owned destination; the ring fixes all source bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnedCopyImageRequest {
    pub slot: OwnedSlotIdentity,
    pub resource: ContentResourceId,
    pub allocation: ContentAllocationId,
    pub output: ContentOutputId,
    pub topology_generation: u64,
    pub width: u32,
    pub height: u32,
}

impl OwnedCopyImageRequest {
    pub fn pixel_bytes(&self) -> Option<usize> {
        self.width
            .checked_mul(self.height)?
            .checked_mul(4)?
            .try_into()
            .ok()
    }
}

#[derive(Debug)]
pub struct OwnedCopyImageMetadata {
    #[cfg(feature = "experimental-native-frame-transaction")]
    pub(crate) frame: Option<crate::shell_content::candidates::FrameCopyBinding>,
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

/// Copy-bearing frame header; scene rows remain in the existing candidate store.
#[cfg(feature = "experimental-native-frame-transaction")]
#[derive(Clone, Debug)]
pub struct OwnedFrameCopyRequest {
    pub transaction: sophia_protocol::TransactionId,
    pub begin: sophia_protocol::ContentCandidateBegin,
    pub image: OwnedCopyImageRequest,
}

#[cfg(feature = "experimental-native-frame-transaction")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnedFrameAdmission {
    Deferred,
    Accepted { job: u64 },
    Refused(crate::ContentCandidateError),
}

#[cfg(feature = "experimental-native-frame-transaction")]
impl OwnedCopyImageJobs {
    /// Take one complete bounded input only after reserving its scene, intent and copy.
    /// The caller supplies connection-wide response capacity, source-release journal
    /// capacity and fresh custody. Deferred/error leaves the input owned by the caller.
    ///
    /// # Errors
    /// Returns Stale for an absent input or foreign grant/source owner, and
    /// ClockRegression for a regressed owner clock. Recoverable admission
    /// failures consume the whole input and return a reserved Refused result.
    pub fn admit_frame_commit(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker,
        ring: Option<&OwnedRing>,
        input: &mut Option<crate::ContentFrameCommit>,
        response_room: usize,
        now: u64,
        deadline: Instant,
    ) -> Result<crate::ContentFrameAdmission, crate::ContentCandidateError> {
        let slot = self.pending.iter_mut().find(|slot| slot.is_none());
        let (decision, admission) = registry.reserve_frame_commit(
            worker,
            ring,
            input,
            response_room,
            slot.is_some(),
            now,
            deadline,
        )?;
        Ok(match decision {
            crate::ContentFrameRequestAdmission::Deferred => crate::ContentFrameAdmission::Deferred,
            crate::ContentFrameRequestAdmission::Refused(error) => {
                crate::ContentFrameAdmission::Refused(error)
            }
            crate::ContentFrameRequestAdmission::Accepted => {
                let copy_job = admission.as_ref().map(|copy| copy.worker_job);
                if let Some(admission) = admission {
                    *slot.expect("copy admission reserved a table slot") = Some(Pending {
                        admission,
                        result: None,
                        failure: None,
                        submitted: false,
                        stopped: None,
                    });
                }
                crate::ContentFrameAdmission::Accepted { copy_job }
            }
        })
    }

    /// Reserve the existing worker, resource and frame owners before consuming input.
    /// The caller retains source-release journal capacity and freshly checks custody.
    /// `response_room` includes all connection-wide record and byte limits.
    pub fn reserve_frame_copy(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker,
        ring: &OwnedRing,
        frame: OwnedFrameCopyRequest,
        response_room: usize,
        now: u64,
        deadline: Instant,
        consume: impl FnOnce(),
    ) -> Result<OwnedFrameAdmission, crate::ContentCandidateError> {
        let slot = self.pending.iter_mut().find(|slot| slot.is_none());
        let (decision, admission) = registry.reserve_owned_frame_copy(
            worker,
            ring,
            frame,
            response_room,
            slot.is_some(),
            now,
            deadline,
            consume,
        )?;
        Ok(match decision {
            crate::ContentFrameRequestAdmission::Deferred => OwnedFrameAdmission::Deferred,
            crate::ContentFrameRequestAdmission::Refused(error) => {
                OwnedFrameAdmission::Refused(error)
            }
            crate::ContentFrameRequestAdmission::Accepted => {
                let admission = admission.expect("accepted frame reserved a copy");
                let job = admission.worker_job;
                *slot.expect("accepted frame reserved a job slot") = Some(Pending {
                    admission,
                    result: None,
                    failure: None,
                    submitted: false,
                    stopped: None,
                });
                OwnedFrameAdmission::Accepted { job }
            }
        })
    }
}

#[cfg(feature = "experimental-native-frame-transaction")]
impl OwnedCopyImageSettlement {
    /// Release the exact frame's copy gate after resource reconciliation.
    /// Errors preserve this settlement for retry; a replacement epoch is never used.
    pub fn finish_frame_copy(
        &mut self,
        registry: &mut ContentEpochRegistry,
        now: u64,
    ) -> Result<(), crate::ContentCandidateError> {
        let Some(binding) = self.admission.metadata.frame.as_ref() else {
            return Ok(());
        };
        if binding.key.grant != self.admission.grant {
            return Err(crate::ContentCandidateError::Stale);
        }
        if let Some(candidates) = registry.candidates_mut(self.admission.grant) {
            let failure = self
                .stopped
                .map(|reason| match reason {
                    ContentReason::Timeout => crate::ContentCandidateError::Timeout,
                    ContentReason::Cancelled => crate::ContentCandidateError::Cancelled,
                    ContentReason::AllocationLost => crate::ContentCandidateError::AllocationLost,
                    _ => crate::ContentCandidateError::Stale,
                })
                .or_else(|| self.outcome.as_ref().err().copied().map(Into::into));
            candidates.finish_frame_copy(binding, self.admission.worker_job, failure, now)?;
        }
        // A missing retired epoch owes no peer record and cannot be retargeted.
        self.admission.metadata.frame = None;
        Ok(())
    }
}

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
