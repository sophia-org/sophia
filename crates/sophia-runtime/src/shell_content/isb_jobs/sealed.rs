//! Sealed-FD ingress for the shared CPU image job lifecycle.
use super::*;
use std::os::fd::OwnedFd;

impl CpuImageJobs<ShellImageMetadata> {
    /// Journal and offer credits must already be held by the channel owner.
    /// No ImageAdmitted is published here. Failure leaves no table entry.
    pub fn reserve(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker,
        grant: ContentGrant,
        metadata: ShellImageMetadata,
        now: u64,
        deadline: Instant,
    ) -> Result<u64, IsbImageAdmissionError> {
        let slot = self
            .pending
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(IsbImageAdmissionError::Worker(IsbSubmitFailure::Capacity))?;
        let admission = registry.reserve_isb_image(grant, metadata, worker, now, deadline)?;
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

    /// The channel validates packet identity and shape before this handoff.
    /// Refusal returns the FD; a withdrawn or already submitted job never sends
    /// a second source to the worker.
    pub fn submit(
        &mut self,
        worker: &mut IsbValidationWorker,
        job: u64,
        source: OwnedFd,
    ) -> Result<(), (IsbSubmitFailure, OwnedFd)> {
        let Some(pending) = self.pending.iter_mut().flatten().find(|pending| {
            pending.admission.worker_job == job
                && !pending.submitted
                && pending.stopped.is_none()
                && pending.result.is_none()
        }) else {
            return Err((IsbSubmitFailure::Invalid, source));
        };
        worker.submit_reserved(job, source)?;
        pending.submitted = true;
        Ok(())
    }
}
