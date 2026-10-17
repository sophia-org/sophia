//! Owner-held job identities outlive the export that admitted them.
use super::*;
use sophia_protocol::shell_files::ShellImageMetadata;
use sophia_protocol::{ContentGrant, ContentReason, ContentResourceId};
use std::os::fd::OwnedFd;
use std::time::Instant;

struct Pending {
    admission: SealedImageAdmission,
    result: Option<Result<ValidatedSealedPixels, ContentStoreError>>,
    failure: Option<SealedValidationFailure>,
    submitted: bool,
    stopped: Option<ContentReason>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SealedImageWithdrawal {
    Cancelled,
    Expired,
    ChannelLost,
    AllocationLost,
}

impl SealedImageWithdrawal {
    fn reason(self) -> ContentReason {
        match self {
            Self::Cancelled | Self::ChannelLost => ContentReason::Cancelled,
            Self::Expired => ContentReason::Timeout,
            Self::AllocationLost => ContentReason::AllocationLost,
        }
    }
}

/// Pair this table with one worker for its entire lifetime and keep both with
/// the resource registry, not with a connection. Worker job IDs are local to
/// that worker. A taken
/// worker completion continues to occupy a slot until its exact resource owner
/// consumes it. In particular, clock regression cannot discard a mapping.
#[derive(Default)]
pub struct SealedImageJobs {
    pending: [Option<Pending>; 4],
}

#[derive(Debug)]
pub struct SealedImageSettlement {
    pub admission: SealedImageAdmission,
    pub outcome: Result<(), ContentStoreError>,
    /// Retained separately from the store outcome for SourceReleased policy.
    pub validation_failure: Option<SealedValidationFailure>,
    /// First owner-side cause; late cancellation cannot overwrite expiry.
    pub stopped: Option<ContentReason>,
}

impl SealedImageJobs {
    /// Journal and offer credits must already be held by the channel owner.
    /// No ImageAdmitted is published here. Failure leaves no table entry.
    pub fn reserve(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut SealedValidationWorker,
        grant: ContentGrant,
        metadata: ShellImageMetadata,
        now: u64,
        deadline: Instant,
    ) -> Result<u64, SealedImageAdmissionError> {
        let slot = self.pending.iter_mut().find(|slot| slot.is_none()).ok_or(
            SealedImageAdmissionError::Worker(SealedSubmitFailure::Capacity),
        )?;
        let admission = registry.reserve_sealed_image(grant, metadata, worker, now, deadline)?;
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
        worker: &mut SealedValidationWorker,
        job: u64,
        source: OwnedFd,
    ) -> Result<(), (SealedSubmitFailure, OwnedFd)> {
        let Some(pending) = self.pending.iter_mut().flatten().find(|pending| {
            pending.admission.worker_job == job
                && !pending.submitted
                && pending.stopped.is_none()
                && pending.result.is_none()
        }) else {
            return Err((SealedSubmitFailure::Invalid, source));
        };
        worker.submit_reserved(job, source)?;
        pending.submitted = true;
        Ok(())
    }

    /// Mark the store before cancelling worker work. Unsent reservations have
    /// no worker completion, so manufacture only their local refusal handoff.
    /// Submitted jobs keep their credit and source until the real completion.
    pub fn withdraw(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut SealedValidationWorker,
        job: u64,
        cause: SealedImageWithdrawal,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        let pending = self
            .pending
            .iter_mut()
            .flatten()
            .find(|pending| pending.admission.worker_job == job)
            .ok_or(ContentStoreError::Stale)?;
        let metadata = &pending.admission.metadata;
        let reason = registry.stop_sealed_image(
            pending.admission.grant,
            ContentResourceId {
                id: metadata.resource,
                generation: metadata.resource_generation,
            },
            cause.reason(),
            now,
        )?;
        pending.stopped = Some(reason);
        if pending.submitted {
            // A result may already be held here; it still must go through the
            // stopped store, which refuses acceptance even if validation won.
            worker.cancel(job);
        } else if pending.result.is_none() {
            if !worker.release_reserved(job) {
                return Err(ContentStoreError::Incomplete);
            }
            pending.result = Some(Err(ContentStoreError::Incomplete));
        }
        Ok(())
    }

    /// Accept exactly one completion for a known job. An unknown or repeated
    /// completion is returned intact; it is never silently dropped or routed
    /// to the newest connection.
    pub fn receive(
        &mut self,
        completion: SealedValidationCompletion,
    ) -> Result<(), SealedValidationCompletion> {
        let Some(pending) = self.pending.iter_mut().flatten().find(|pending| {
            pending.admission.worker_job == completion.job
                && pending.submitted
                && pending.result.is_none()
        }) else {
            return Err(completion);
        };
        pending.failure = completion.result.as_ref().err().copied();
        pending.result = Some(completion.result.map_err(|_| ContentStoreError::Malformed));
        Ok(())
    }

    /// Earliest original admission deadline. Receiving Source never renews it.
    pub fn deadline(&self) -> Option<Instant> {
        self.pending
            .iter()
            .flatten()
            .filter(|pending| pending.stopped.is_none())
            .map(|pending| pending.admission.deadline)
            .min()
    }

    pub fn expire(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut SealedValidationWorker,
        instant: Instant,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        let jobs = self.pending.each_ref().map(|slot| {
            slot.as_ref()
                .filter(|pending| {
                    pending.stopped.is_none() && instant >= pending.admission.deadline
                })
                .map(|pending| pending.admission.worker_job)
        });
        for job in jobs.into_iter().flatten() {
            self.withdraw(registry, worker, job, SealedImageWithdrawal::Expired, now)?;
        }
        Ok(())
    }

    /// Channel loss withdraws every offer, including sources that never arrived.
    /// Run this after registry disconnect too: stopped retired transfers remain
    /// reachable, but no old journal can acquire a new event obligation.
    pub fn withdraw_epoch(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut SealedValidationWorker,
        grant: ContentGrant,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        let jobs = self.pending.each_ref().map(|slot| {
            slot.as_ref()
                .filter(|pending| pending.admission.grant == grant)
                .map(|pending| pending.admission.worker_job)
        });
        for job in jobs.into_iter().flatten() {
            self.withdraw(
                registry,
                worker,
                job,
                SealedImageWithdrawal::ChannelLost,
                now,
            )?;
        }
        Ok(())
    }

    /// Reconcile one ready job. An error is a settlement only when the store
    /// consumed the result. Otherwise the slot and mapping stay here for retry.
    /// Search all slots so one mistimed owner cannot block another epoch.
    pub fn reconcile(
        &mut self,
        registry: &mut ContentEpochRegistry,
        now: u64,
    ) -> Option<SealedImageSettlement> {
        for slot in &mut self.pending {
            let Some(pending) = slot.as_mut().filter(|pending| pending.result.is_some()) else {
                continue;
            };
            let metadata = &pending.admission.metadata;
            // A validation deadline can precede the transfer timeout. Preserve
            // that timeout as a store outcome instead of reporting bad pixels.
            if pending.stopped.is_none()
                && matches!(
                    pending.failure,
                    Some(SealedValidationFailure::Timeout | SealedValidationFailure::Cancelled)
                )
            {
                let reason = if pending.failure == Some(SealedValidationFailure::Timeout) {
                    ContentReason::Timeout
                } else {
                    ContentReason::Cancelled
                };
                match registry.stop_sealed_image(
                    pending.admission.grant,
                    ContentResourceId {
                        id: metadata.resource,
                        generation: metadata.resource_generation,
                    },
                    reason,
                    now,
                ) {
                    Ok(reason) => pending.stopped = Some(reason),
                    Err(_) => continue,
                }
            }
            let outcome = registry.complete_sealed(
                pending.admission.grant,
                ContentResourceId {
                    id: metadata.resource,
                    generation: metadata.resource_generation,
                },
                &mut pending.result,
                now,
            );
            if pending.result.is_none() {
                let pending = slot.take().expect("consumed job occupies its slot");
                return Some(SealedImageSettlement {
                    admission: pending.admission,
                    outcome,
                    validation_failure: pending.failure,
                    stopped: pending.stopped,
                });
            }
        }
        None
    }

    pub fn pending(&self) -> usize {
        self.pending.iter().flatten().count()
    }
}
