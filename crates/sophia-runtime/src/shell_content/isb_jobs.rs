//! Owner-held job identities outlive the export that admitted them.
use super::*;
use sophia_protocol::shell_files::ShellImageMetadata;
use sophia_protocol::{ContentGrant, ContentReason, ContentResourceId};
use std::time::Instant;

struct Pending<M> {
    admission: CpuImageAdmission<M>,
    result: Option<Result<ValidatedIsbPixels, ContentStoreError>>,
    failure: Option<IsbValidationFailure>,
    submitted: bool,
    stopped: Option<ContentReason>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IsbImageWithdrawal {
    Cancelled,
    Expired,
    ChannelLost,
    AllocationLost,
}

impl IsbImageWithdrawal {
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
pub struct CpuImageJobs<M> {
    pending: [Option<Pending<M>>; 4],
}

impl<M> Default for CpuImageJobs<M> {
    fn default() -> Self {
        Self {
            pending: std::array::from_fn(|_| None),
        }
    }
}

pub type IsbImageJobs = CpuImageJobs<ShellImageMetadata>;
pub type IsbImageSettlement = CpuImageSettlement<ShellImageMetadata>;
mod owned_copy;
mod sealed;
pub use owned_copy::*;

pub(crate) mod metadata_sealed {
    pub trait Sealed {}
}

/// Only the runtime's sealed-FD and owned-copy admission types implement this.
/// It shares resource custody, not transport or source authentication.
pub trait CpuImageMetadata: metadata_sealed::Sealed {
    fn resource(&self) -> ContentResourceId;
    /// Transport owners may already perform this check. Owned-copy admission
    /// additionally rechecks its exact allocation before accepting a result.
    fn allocation_current(&self, _registry: &ContentEpochRegistry, _grant: ContentGrant) -> bool {
        true
    }
}
impl metadata_sealed::Sealed for ShellImageMetadata {}
impl CpuImageMetadata for ShellImageMetadata {
    fn resource(&self) -> ContentResourceId {
        ContentResourceId {
            id: self.resource,
            generation: self.resource_generation,
        }
    }
}

#[derive(Debug)]
pub struct CpuImageSettlement<M> {
    pub admission: CpuImageAdmission<M>,
    pub outcome: Result<(), ContentStoreError>,
    /// Retained separately from the store outcome for SourceReleased policy.
    pub validation_failure: Option<IsbValidationFailure>,
    /// First owner-side cause; late cancellation cannot overwrite expiry.
    pub stopped: Option<ContentReason>,
}

impl<M: CpuImageMetadata> CpuImageJobs<M> {
    /// Mark the store before cancelling worker work. Unsent reservations have
    /// no worker completion, so manufacture only their local refusal handoff.
    /// Submitted jobs keep their credit and source until the real completion.
    /// An Incomplete error releasing an unsent credit indicates the wrong
    /// worker was supplied. Custody stays stopped here; retry with the paired
    /// worker. Never discard the slot or synthesize a second completion.
    pub fn withdraw(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker,
        job: u64,
        cause: IsbImageWithdrawal,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        let pending = self
            .pending
            .iter_mut()
            .flatten()
            .find(|pending| pending.admission.worker_job == job)
            .ok_or(ContentStoreError::Stale)?;
        let metadata = &pending.admission.metadata;
        let reason = registry.stop_isb_image(
            pending.admission.grant,
            metadata.resource(),
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
        completion: IsbValidationCompletion,
    ) -> Result<(), IsbValidationCompletion> {
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
            .filter(|pending| pending.stopped.is_none() && pending.result.is_none())
            .map(|pending| pending.admission.deadline)
            .min()
    }

    pub fn expire(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker,
        instant: Instant,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        let jobs = self.pending.each_ref().map(|slot| {
            slot.as_ref()
                .filter(|pending| {
                    pending.stopped.is_none()
                        && pending.result.is_none()
                        && instant >= pending.admission.deadline
                })
                .map(|pending| pending.admission.worker_job)
        });
        let mut first_error = None;
        for job in jobs.into_iter().flatten() {
            if let Err(error) =
                self.withdraw(registry, worker, job, IsbImageWithdrawal::Expired, now)
            {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Channel loss withdraws every offer, including sources that never arrived.
    /// Run this after registry disconnect too: stopped retired transfers remain
    /// reachable, but no old journal can acquire a new event obligation.
    pub fn withdraw_epoch(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker,
        grant: ContentGrant,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        let jobs = self.pending.each_ref().map(|slot| {
            slot.as_ref()
                .filter(|pending| pending.admission.grant == grant)
                .map(|pending| pending.admission.worker_job)
        });
        let mut first_error = None;
        for job in jobs.into_iter().flatten() {
            if let Err(error) =
                self.withdraw(registry, worker, job, IsbImageWithdrawal::ChannelLost, now)
            {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Reconcile one ready job. An error is a settlement only when the store
    /// consumed the result. Otherwise the slot and mapping stay here for retry.
    /// Search all slots so one mistimed owner cannot block another epoch.
    pub fn reconcile(
        &mut self,
        registry: &mut ContentEpochRegistry,
        now: u64,
    ) -> Option<CpuImageSettlement<M>> {
        crate::shell_work_cost::option(crate::shell_work_cost::Stage::Settle, || {
            for slot in &mut self.pending {
                let Some(pending) = slot.as_mut().filter(|pending| pending.result.is_some()) else {
                    continue;
                };
                let metadata = &pending.admission.metadata;
                if pending.stopped.is_none()
                    && !metadata.allocation_current(registry, pending.admission.grant)
                {
                    match registry.stop_isb_image(
                        pending.admission.grant,
                        metadata.resource(),
                        ContentReason::AllocationLost,
                        now,
                    ) {
                        Ok(reason) => pending.stopped = Some(reason),
                        Err(_) => continue,
                    }
                }
                // A validation deadline can precede the transfer timeout. Preserve
                // that timeout as a store outcome instead of reporting bad pixels.
                if pending.stopped.is_none()
                    && matches!(
                        pending.failure,
                        Some(IsbValidationFailure::Timeout | IsbValidationFailure::Cancelled)
                    )
                {
                    let reason = if pending.failure == Some(IsbValidationFailure::Timeout) {
                        ContentReason::Timeout
                    } else {
                        ContentReason::Cancelled
                    };
                    match registry.stop_isb_image(
                        pending.admission.grant,
                        metadata.resource(),
                        reason,
                        now,
                    ) {
                        Ok(reason) => pending.stopped = Some(reason),
                        Err(_) => continue,
                    }
                }
                let outcome = registry.complete_isb(
                    pending.admission.grant,
                    metadata.resource(),
                    &mut pending.result,
                    now,
                );
                if pending.result.is_none() {
                    let pending = slot.take().expect("consumed job occupies its slot");
                    return Some(CpuImageSettlement {
                        admission: pending.admission,
                        outcome,
                        validation_failure: pending.failure,
                        stopped: pending.stopped,
                    });
                }
            }
            None
        })
    }

    pub fn pending(&self) -> usize {
        self.pending.iter().flatten().count()
    }

    pub fn admissions(&self) -> impl Iterator<Item = &CpuImageAdmission<M>> {
        self.pending
            .iter()
            .flatten()
            .map(|pending| &pending.admission)
    }

    /// Bridge store-side ResourceCancel, expiry and revocation to worker
    /// custody. Run after resource service and before taking new Sources.
    /// A stopped unsent offer needs a local handoff even if no FD ever arrives.
    /// The store's original cause wins; the fallback cause cannot replace it.
    pub fn observe_store_stops(
        &mut self,
        registry: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        let jobs = self.pending.each_ref().map(|slot| {
            let pending = slot.as_ref()?;
            let admission = &pending.admission;
            registry.isb_stop_reason(admission.grant, admission.metadata.resource())?;
            Some(admission.worker_job)
        });
        let mut first_error = None;
        for job in jobs.into_iter().flatten() {
            if let Err(error) =
                self.withdraw(registry, worker, job, IsbImageWithdrawal::Cancelled, now)
            {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}
