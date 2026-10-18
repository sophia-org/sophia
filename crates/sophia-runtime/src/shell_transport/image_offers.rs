//! Explicit resource-owner hooks for the authenticated shell image channel.
use super::{ShellComponentTransport, ShellTransportError};
use crate::{ContentEpochRegistry, IsbImageJobs, IsbImageSettlement, IsbValidationWorker};
use sophia_protocol::ContentReason;
use sophia_protocol::shell_files::{ShellImageMetadata, ShellSourceDisposition};
use std::os::fd::OwnedFd;
use std::time::Instant;

fn wire_error(error: sophia_9p::Errno) -> ShellTransportError {
    if error == sophia_9p::Errno::EAGAIN {
        ShellTransportError::ContentQueueSaturated
    } else if error == sophia_9p::Errno::ESTALE {
        ShellTransportError::WrongContentGrant
    } else if error == sophia_9p::Errno::EACCES {
        ShellTransportError::MissingCapability
    } else if error == sophia_9p::Errno::EINVAL {
        ShellTransportError::WrongContentRecord
    } else {
        ShellTransportError::Io(format!("image offer: {error:?}"))
    }
}

impl ShellComponentTransport {
    /// False after channel closure or epoch loss. The owner must then withdraw
    /// this grant's pending jobs even if the 9P byte path remains connected.
    pub fn isb_image_channel_ready(&self) -> bool {
        self.files().is_some_and(|files| {
            matches!(
                files.export().image_readiness(),
                super::files::ImageReadiness::Ready {
                    granted_source_kinds: 1
                }
            )
        })
    }
    /// Explicit owner opt-in after authenticated ChannelReady. The owner must
    /// service pending offers, Sources, deadlines and completions every turn;
    /// configuration alone continues to select the receive/refuse behavior.
    pub fn enable_isb_image_admission(&mut self) -> Result<(), ShellTransportError> {
        self.files_mut()
            .ok_or(ShellTransportError::NotConnected)?
            .export_mut()
            .enable_image_offers()
            .map_err(wire_error)
    }

    pub fn pending_isb_image_offer(&self) -> Option<ShellImageMetadata> {
        self.files()?.export().pending_image_offer()
    }

    /// The exact oldest ImageBegin already has Submitted custody. Reserve its
    /// terminal records, live resource and worker credit before ImageAdmitted.
    /// Failure leaves it pending for a later retry or explicit rejection.
    /// The owner must reject permanent failures, and queued offers after
    /// channel closure, rather than retrying them indefinitely. Transient
    /// capacity retries must be bounded by the owner's offer deadline.
    pub fn admit_pending_isb_image(
        &mut self,
        epochs: &mut ContentEpochRegistry,
        jobs: &mut IsbImageJobs,
        worker: &mut IsbValidationWorker,
        now_msec: u64,
        now: Instant,
    ) -> Result<u64, ShellTransportError> {
        let grant = self.store_grant;
        let files = self.files_mut().ok_or(ShellTransportError::NotConnected)?;
        let timeout = files
            .export()
            .image_validation_timeout()
            .ok_or(ShellTransportError::MissingCapability)?;
        let deadline = now
            .checked_add(timeout)
            .ok_or(ShellTransportError::WrongContentGrant)?;
        let job = files
            .export_mut()
            .admit_image_offer(grant, epochs, jobs, worker, now_msec, deadline)
            .map_err(wire_error)?;
        files.image_publication_pending();
        Ok(job)
    }

    pub fn refuse_pending_isb_image(
        &mut self,
        reason: ContentReason,
    ) -> Result<(), ShellTransportError> {
        let files = self.files_mut().ok_or(ShellTransportError::NotConnected)?;
        files
            .export_mut()
            .refuse_image_offer(reason)
            .map_err(wire_error)?;
        files.image_publication_pending();
        Ok(())
    }

    /// Validated packet shape and live offer identity, not validated pixels.
    /// Submit it to the paired jobs/worker; a refused handoff must drop the FD
    /// and withdraw its job. No source can be taken twice.
    pub fn take_isb_image_source(&mut self) -> Option<(u64, OwnedFd)> {
        self.files_mut()?.export_mut().take_image_source()
    }

    /// Publish only after the resource owner consumed the worker result. A
    /// failed publication leaves the caller owning this settlement for retry.
    /// The ordinary resource-event drain spends the independent status credit.
    /// ContentQueueSaturated also means that status has not reached the journal
    /// yet; retain the settlement and retry after resource/output progress.
    pub fn release_isb_image_source(
        &mut self,
        settlement: &IsbImageSettlement,
    ) -> Result<u64, ShellTransportError> {
        if settlement.admission.grant != self.store_grant {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let disposition = if settlement.outcome.is_ok() {
            ShellSourceDisposition::Retained
        } else if matches!(
            settlement.stopped,
            Some(
                ContentReason::Cancelled
                    | ContentReason::Revoked
                    | ContentReason::AllocationLost
                    | ContentReason::OutputLost
            )
        ) {
            ShellSourceDisposition::CancelledBeforeImport
        } else {
            ShellSourceDisposition::RefusedBeforeUse
        };
        let files = self.files_mut().ok_or(ShellTransportError::NotConnected)?;
        let sequence = files
            .export_mut()
            .release_image_source(settlement.admission.metadata, disposition)
            .map_err(wire_error)?;
        files.image_publication_pending();
        Ok(sequence)
    }
}
