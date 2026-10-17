//! Owner-held job identities outlive the export that admitted them.
use super::*;
use sophia_protocol::shell_files::ShellImageMetadata;
use sophia_protocol::{ContentGrant, ContentResourceId};
use std::time::Instant;

struct Pending {
    admission: SealedImageAdmission,
    result: Option<Result<ValidatedSealedPixels, ContentStoreError>>,
    failure: Option<SealedValidationFailure>,
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
        });
        Ok(job)
    }

    /// Accept exactly one completion for a known job. An unknown or repeated
    /// completion is returned intact; it is never silently dropped or routed
    /// to the newest connection.
    pub fn receive(
        &mut self,
        completion: SealedValidationCompletion,
    ) -> Result<(), SealedValidationCompletion> {
        let Some(pending) = self.pending.iter_mut().flatten().find(|pending| {
            pending.admission.worker_job == completion.job && pending.result.is_none()
        }) else {
            return Err(completion);
        };
        pending.failure = completion.result.as_ref().err().copied();
        pending.result = Some(completion.result.map_err(|_| ContentStoreError::Malformed));
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
                });
            }
        }
        None
    }

    pub fn pending(&self) -> usize {
        self.pending.iter().flatten().count()
    }
}
