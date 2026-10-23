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

/// An admitted DMA-BUF offer. ImageAdmitted is journaled and the logical
/// capture credit is held; the caller must settle it exactly once, through
/// the capture owner or `fail_capture`, before SourceReleased.
#[cfg(feature = "experimental-shell-capture")]
#[derive(Debug)]
#[must_use = "Settle the capture reservation; dropping it strands its credit"]
pub struct ShellCaptureAdmission {
    pub metadata: ShellImageMetadata,
    pub reservation: crate::ContentCaptureReservation,
    /// Fixed when ImageBegin was accepted; Source wait cannot extend it.
    pub deadline: Instant,
}

/// One received DMA-BUF source with every fact its capture binding needs.
#[cfg(feature = "experimental-shell-capture")]
#[derive(Debug)]
pub struct ShellCaptureImageSource {
    pub metadata: ShellImageMetadata,
    pub plane: sophia_protocol::shell_files::ShellImagePlane,
    /// The descriptor's `lseek(SEEK_END)` size at receipt. The plane's LINEAR
    /// read span fits inside it and it is charged to `max_dma_source_bytes`.
    pub source_bytes: u64,
    pub fd: OwnedFd,
}

#[cfg(feature = "experimental-shell-capture")]
impl ShellComponentTransport {
    /// Deliver capture journal publications without accepting wire requests or
    /// image packets. Deferred channel publications wait for ordinary service.
    /// Quiet calls do no transport work; all flushes retain custody checks.
    pub fn flush_capture_publications(&mut self) -> Result<(), ShellTransportError> {
        let files = self.files_mut().ok_or(ShellTransportError::NotConnected)?;
        if files.publication_pending() {
            files.flush_publications()?;
        }
        Ok(())
    }

    /// False after channel closure or epoch loss, or when DMA-BUF was not
    /// granted. The owner must then withdraw this grant's admissions.
    pub fn capture_image_channel_ready(&self) -> bool {
        self.wire.as_ref().is_some_and(|files| {
            matches!(
                files.export().image_readiness(),
                super::files::ImageReadiness::Ready { granted_source_kinds }
                    if granted_source_kinds
                        & sophia_protocol::shell_files::SHELL_IMAGE_SOURCE_DMA_BUF
                        != 0
            )
        })
    }

    /// Explicit owner opt-in after ChannelReady granted DMA-BUF. Until then a
    /// DMA-BUF ImageBegin waits with its custody preserved.
    pub fn enable_capture_image_admission(&mut self) -> Result<(), ShellTransportError> {
        self.files_mut()
            .ok_or(ShellTransportError::NotConnected)?
            .export_mut()
            .enable_capture_image_offers()
            .map_err(wire_error)
    }

    /// The oldest queued offer when it is DMA-BUF. The deadline was fixed at
    /// ImageBegin custody; refuse the offer once it passes.
    pub fn pending_capture_image_offer(&self) -> Option<(ShellImageMetadata, Instant)> {
        self.wire.as_ref()?.export().pending_capture_image_offer()
    }

    /// Hold the physical renderer ticket first and pass its byte bound. This
    /// checks the format row and DMA-BUF limits, holds both terminal journal
    /// credits and reserves logical capture credit before ImageAdmitted.
    /// ContentQueueSaturated leaves the offer queued for a bounded retry.
    pub fn admit_capture_image_offer(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        renderer_allocation_bound: u64,
        now_msec: u64,
    ) -> Result<ShellCaptureAdmission, ShellTransportError> {
        let grant = self.store_grant;
        let files = self.files_mut().ok_or(ShellTransportError::NotConnected)?;
        let (metadata, reservation, deadline) = files
            .export_mut()
            .admit_capture_image_offer(grant, epochs, renderer_allocation_bound, now_msec)
            .map_err(wire_error)?;
        files.image_publication_pending();
        Ok(ShellCaptureAdmission {
            metadata,
            reservation,
            deadline,
        })
    }

    /// Shape, LINEAR span and exact descriptor size are checked; pixels and
    /// device are not. The caller binds it to its admission before use.
    pub fn take_capture_image_source(&mut self) -> Option<ShellCaptureImageSource> {
        self.files_mut()?.export_mut().take_capture_image_source()
    }
}

impl ShellComponentTransport {
    /// Trusted capture-owner publication AFTER exact-job completion and resource
    /// settlement. This is journal accounting, not a GPU completion verifier.
    /// Session retains the opaque native receipt/source on every refusal.
    /// The export independently requires matching metadata and journaled status.
    #[cfg(feature = "experimental-shell-capture")]
    pub fn release_capture_image_source(
        &mut self,
        epochs: &mut ContentEpochRegistry,
        grant: sophia_protocol::ContentGrant,
        metadata: ShellImageMetadata,
        disposition: ShellSourceDisposition,
    ) -> Result<u64, ShellTransportError> {
        if grant != self.store_grant || metadata.admission_epoch != grant.content_grant_epoch {
            return Err(ShellTransportError::WrongContentGrant);
        }
        if metadata.source_kind != sophia_protocol::shell_files::SHELL_IMAGE_SOURCE_DMA_BUF
            || disposition == ShellSourceDisposition::Retained
        {
            return Err(ShellTransportError::WrongContentRecord);
        }
        let released = self
            .files_mut()
            .ok_or(ShellTransportError::NotConnected)?
            .export_mut()
            .release_image_source(metadata, disposition);
        let sequence = if released == Err(sophia_9p::Errno::EAGAIN) {
            // The exact offer still owes status. Move the grant's store events
            // and whole outbox in FIFO order, without wire service, before retry.
            self.flush_content_resource_events(epochs)?;
            let files = self
                .wire
                .as_mut()
                .ok_or(ShellTransportError::NotConnected)?;
            self.output_blocked = !Self::drain_file_output(
                files,
                &mut self.output,
                epochs.resources_mut(self.store_grant),
            )?;
            files
                .export_mut()
                .release_image_source(metadata, disposition)
                .map_err(wire_error)?
        } else {
            released.map_err(wire_error)?
        };
        self.wire
            .as_mut()
            .expect("release requires connected wire")
            .image_publication_pending();
        Ok(sequence)
    }

    /// False after channel closure or epoch loss. The owner must then withdraw
    /// this grant's pending jobs even if the 9P byte path remains connected.
    pub fn isb_image_channel_ready(&self) -> bool {
        self.wire.as_ref().is_some_and(|files| {
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

    /// Never a DMA-BUF offer; those belong to the capture owner.
    pub fn pending_isb_image_offer(&self) -> Option<ShellImageMetadata> {
        self.wire.as_ref()?.export().pending_cpu_image_offer()
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
        self.refuse_pending_image(reason)
    }

    /// Refuses the oldest queued offer of either source kind.
    pub fn refuse_pending_image(
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
