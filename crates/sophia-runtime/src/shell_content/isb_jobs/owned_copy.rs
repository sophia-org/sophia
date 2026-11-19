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
    pub fn admit_frame_commit<P: Send + 'static>(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker<P>,
        ring: Option<&OwnedRing>,
        input: &mut Option<crate::ContentFrameCommit>,
        response_room: usize,
        now: u64,
        deadline: Instant,
    ) -> Result<crate::ContentFrameAdmission, crate::ContentCandidateError> {
        self.admit_frame_commit_with_source(
            registry,
            worker,
            ring,
            input,
            response_room,
            None,
            now,
            deadline,
            #[cfg(any(test, feature = "experimental-combined-frame-job"))]
            None,
        )
    }

    pub(crate) fn admit_frame_commit_with_source<P: Send + 'static>(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker<P>,
        ring: Option<&OwnedRing>,
        input: &mut Option<crate::ContentFrameCommit>,
        response_room: usize,
        source_error: Option<crate::ContentCandidateError>,
        now: u64,
        deadline: Instant,
        #[cfg(any(test, feature = "experimental-combined-frame-job"))] combined: Option<
            &mut Option<crate::CombinedReservation>,
        >,
    ) -> Result<crate::ContentFrameAdmission, crate::ContentCandidateError> {
        let slot = self.pending.iter_mut().find(|slot| slot.is_none());
        let (decision, admission) = registry.reserve_frame_commit(
            worker,
            ring,
            input,
            response_room,
            slot.is_some(),
            source_error,
            now,
            deadline,
            #[cfg(any(test, feature = "experimental-combined-frame-job"))]
            combined,
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
    pub fn reserve_frame_copy<P: Send + 'static>(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker<P>,
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
    pub fn reserve<P: Send + 'static>(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker<P>,
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
    pub fn submit<P: Send + 'static>(
        &mut self,
        worker: &mut IsbValidationWorker<P>,
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

/// What the plan hook needs about one admitted combined copy, read from its
/// row: the host-derived description and the exact candidate binding.
#[cfg(all(
    feature = "experimental-native-frame-transaction",
    any(test, feature = "experimental-combined-frame-job")
))]
pub struct CombinedFrameAdmitted<'a> {
    pub job: u64,
    pub description: &'a sophia_protocol::ContentResourceBegin,
    pub binding: crate::FrameCopyBinding,
    /// The job's original validation deadline; the renderer's plan expires
    /// with it.
    pub deadline: Instant,
}

#[cfg(all(
    feature = "experimental-native-frame-transaction",
    any(test, feature = "experimental-combined-frame-job")
))]
impl OwnedCopyImageJobs {
    /// `admit_frame_commit` whose copy takes one combined credit. The
    /// reservation comes back beside the admission, and only an accepted copy
    /// keeps it: every other outcome already released that credit.
    #[doc(hidden)]
    pub fn admit_frame_commit_combined<P: Send + 'static>(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker<P>,
        ring: Option<&OwnedRing>,
        input: &mut Option<crate::ContentFrameCommit>,
        response_room: usize,
        now: u64,
        deadline: Instant,
    ) -> Result<
        (
            crate::ContentFrameAdmission,
            Option<crate::CombinedReservation>,
        ),
        crate::ContentCandidateError,
    > {
        let mut reservation = None;
        let admission = self.admit_frame_commit_with_source(
            registry,
            worker,
            ring,
            input,
            response_room,
            None,
            now,
            deadline,
            Some(&mut reservation),
        )?;
        let kept = match admission {
            crate::ContentFrameAdmission::Accepted {
                copy_job: Some(job),
            } => reservation.filter(|reservation| reservation.job() == job),
            _ => None,
        };
        Ok((admission, kept))
    }

    /// The row of an admitted, unsent frame copy.
    pub fn combined_admitted(&self, job: u64) -> Option<CombinedFrameAdmitted<'_>> {
        let pending = self.pending.iter().flatten().find(|pending| {
            pending.admission.worker_job == job && !pending.submitted && pending.result.is_none()
        })?;
        Some(CombinedFrameAdmitted {
            job,
            description: &pending.admission.metadata.description,
            binding: pending.admission.metadata.frame?,
            deadline: pending.admission.deadline,
        })
    }

    /// `submit` with the combined send: the same exact binding check, then
    /// the row's own admitted description travels with the job. The table's
    /// flag moves only after the worker accepted the send, as the worker's
    /// own does. A refusal returns the ticket and the plan intact.
    pub fn submit_combined<P: Send + 'static>(
        &mut self,
        worker: &mut IsbValidationWorker<P>,
        reservation: &crate::CombinedReservation,
        source: OwnedRingRead,
        plan: P,
        inbox: crate::CombinedInbox<P>,
    ) -> Result<std::sync::mpsc::Receiver<P>, (IsbSubmitFailure, OwnedRingRead, P)> {
        let job = reservation.job();
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
            return Err((IsbSubmitFailure::Invalid, source, plan));
        };
        let description = pending.admission.metadata.description.clone();
        let returned =
            worker.submit_ring_combined(reservation, source, description, plan, inbox)?;
        pending.submitted = true;
        Ok(returned)
    }
}
