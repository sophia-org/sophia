//! Prototype commands use the existing worker, immutable store and status journal.
//! A pending job remains here after disconnect until its exact epoch settles it.
use super::{ShellComponentTransport, ShellTransportError};
use crate::{
    ContentEpochRegistry, ContentStoreError, IsbValidationCompletion, IsbValidationWorker,
    ValidatedIsbPixels,
};
use sophia_protocol::{
    ContentGrant, ContentReason, ContentResourceId, ContentResourceStatus, ShellContentRecord,
};
use std::time::Instant;

struct Pending {
    job: u64,
    grant: ContentGrant,
    resource: ContentResourceId,
    result: Option<Result<ValidatedIsbPixels, ContentStoreError>>,
    deadline: Instant,
    stopped: bool,
}
#[derive(Default)]
pub(super) struct DrawJobs {
    jobs: [Option<Pending>; 4],
}
impl DrawJobs {
    pub fn deadline(&self) -> Option<Instant> {
        self.jobs
            .iter()
            .flatten()
            .filter(|p| !p.stopped)
            .map(|p| p.deadline)
            .min()
    }
    pub fn pending(&self) -> usize {
        self.jobs.iter().flatten().count()
    }
    pub fn receive(
        &mut self,
        completion: IsbValidationCompletion,
    ) -> Result<(), IsbValidationCompletion> {
        let Some(job) = self
            .jobs
            .iter_mut()
            .flatten()
            .find(|p| p.job == completion.job && p.result.is_none())
        else {
            return Err(completion);
        };
        job.result = Some(completion.result.map_err(|_| ContentStoreError::Malformed));
        Ok(())
    }
    pub fn service(
        &mut self,
        transport: &mut ShellComponentTransport,
        epochs: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker,
        now: u64,
        instant: Instant,
    ) -> Result<(), ShellTransportError> {
        for slot in &mut self.jobs {
            let Some(pending) = slot else { continue };
            if instant >= pending.deadline && !pending.stopped {
                epochs.stop_isb_image(
                    pending.grant,
                    pending.resource,
                    ContentReason::Timeout,
                    now,
                )?;
                pending.stopped = true;
                worker.cancel(pending.job);
            }
            if transport.content_grant() != Some(pending.grant)
                || epochs
                    .isb_stop_reason(pending.grant, pending.resource)
                    .is_some()
            {
                pending.stopped = true;
                worker.cancel(pending.job);
            }
            if pending.result.is_some() {
                let _ =
                    epochs.complete_isb(pending.grant, pending.resource, &mut pending.result, now);
                // Only the store consuming the completion releases our slot.
                if pending.result.is_none() {
                    *slot = None;
                }
            }
        }
        let Some((batch, deadline)) = transport
            .files_mut()
            .and_then(|f| f.export().pending_draw().cloned())
        else {
            return Ok(());
        };
        // One queued batch is bounded separately by the export. Do not consume
        // it until an ordinary terminal status and release can be accounted.
        if !transport.control_capacity_available(epochs, 2)
            || worker.pending() >= 4
            || self.pending() >= 4
        {
            return Ok(());
        }
        if transport.content_grant() != Some(batch.grant) {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let job = if instant < deadline {
            match worker.reserve(batch.pixel_bytes(), deadline) {
                Ok(id) => Some(id),
                Err(crate::IsbSubmitFailure::Capacity) => return Ok(()),
                Err(_) => None,
            }
        } else {
            None
        };
        let result = if job.is_some() {
            batch
                .description(
                    transport
                        .content_limits()
                        .ok_or(ShellTransportError::MissingCapability)?,
                )
                .map_err(|_| ContentStoreError::Malformed)
                .and_then(|description| {
                    epochs
                        .resources_mut(batch.grant)
                        .expect("active content grant")
                        .begin_isb(batch.transaction_id(), description, now)
                })
        } else {
            Err(ContentStoreError::Incomplete)
        };
        if let Err(error) = result {
            if let Some(job) = job {
                worker.release_reserved(job);
            }
            let reason = match error {
                ContentStoreError::Budget => ContentReason::Budget,
                ContentStoreError::Stale => ContentReason::Stale,
                ContentStoreError::Incomplete => ContentReason::Timeout,
                _ => ContentReason::Malformed,
            };
            transport.send_content_record(
                epochs,
                batch.transaction_id(),
                &ShellContentRecord::ResourceStatus(ContentResourceStatus {
                    grant: batch.grant,
                    resource: batch.resource,
                    status: 3,
                    reason: reason as u16,
                    next_ordinal: 0,
                    admitted_bytes: 0,
                }),
            )?;
            transport
                .files_mut()
                .expect("draw export checked")
                .export_mut()
                .take_draw();
            return Ok(());
        }
        transport
            .files_mut()
            .expect("draw export checked")
            .export_mut()
            .take_draw();
        let job = job.expect("successful reservation");
        let mut pending = Pending {
            job,
            grant: batch.grant,
            resource: batch.resource,
            result: None,
            deadline,
            stopped: false,
        };
        if worker.submit_draw_reserved(job, batch).is_err() {
            worker.release_reserved(job);
            pending.result = Some(Err(ContentStoreError::Incomplete));
        }
        *self
            .jobs
            .iter_mut()
            .find(|s| s.is_none())
            .expect("bounded draw credit") = Some(pending);
        Ok(())
    }
}
