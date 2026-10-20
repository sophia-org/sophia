//! Logical resource credit, independent of physical renderer allocation credit.
//! Snapshot custody is storage, never a source-read completion proof.
use super::*;
use sophia_protocol::shell_files::ShellImageMetadata;

/// Exact owner/transfer identity. Not Clone; dropping it does not return credit.
/// The registry must retain the transfer through uncertain GPU work. This is
/// accounting authority, never evidence that source reads have ended.
///
/// ```compile_fail
/// use sophia_runtime::ContentCaptureReservation;
/// fn duplicate(ticket: &ContentCaptureReservation) -> ContentCaptureReservation {
///     ticket.clone()
/// }
/// ```
#[derive(Debug)]
#[must_use = "Retain capture credit until exact-job settlement, including after withdrawal"]
pub struct ContentCaptureReservation {
    description: ContentResourceBegin,
    metadata: ShellImageMetadata,
    identity: Arc<()>,
    allocation: Option<Box<crate::ContentAllocationSnapshot>>,
}

impl ContentCaptureReservation {
    pub(in crate::shell_content) fn bind_allocation(
        &mut self,
        allocation: crate::ContentAllocationSnapshot,
    ) {
        self.allocation = Some(Box::new(allocation));
    }
    pub(in crate::shell_content) fn allocation(&self) -> Option<&crate::ContentAllocationSnapshot> {
        self.allocation.as_deref()
    }
    pub fn description(&self) -> &ContentResourceBegin {
        &self.description
    }
    pub fn metadata(&self) -> &ShellImageMetadata {
        &self.metadata
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentCaptureFailure {
    Renderer,
    Cancelled,
    Timeout,
    Withdrawn,
}

impl ContentCaptureFailure {
    fn reason(self) -> ContentReason {
        match self {
            Self::Renderer => ContentReason::RendererFailed,
            Self::Cancelled => ContentReason::Cancelled,
            Self::Timeout => ContentReason::Timeout,
            Self::Withdrawn => ContentReason::Revoked,
        }
    }
}

impl ContentResourceStore {
    /// Trusted resource-owner acceptance AFTER the opaque native receipt proved
    /// the exact copy complete. This lower accounting API does not validate GPU
    /// execution. Its caller must also revalidate allocation/admission (the
    /// registry entry below does that); it never emits SourceReleased.
    /// Refusal preserves both ticket and snapshot for the completion owner.
    pub(in crate::shell_content) fn accept_capture(
        &mut self,
        reservation: &mut Option<ContentCaptureReservation>,
        image: &mut Option<Arc<ContentCapturedImage>>,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        let held = reservation.as_ref().ok_or(ContentStoreError::Stale)?;
        self.capture_transfer(held)?;
        self.expire(now)?;
        let transfer = self.capture_transfer(held)?;
        if self.revoked || transfer.stopped.is_some() {
            return Err(ContentStoreError::Revoked);
        }
        let snapshot = image.as_ref().ok_or(ContentStoreError::Incomplete)?;
        let frame = snapshot.as_frame();
        if !snapshot.is_retained() || !snapshot.is_current() || !snapshot.image_id().is_valid() {
            return Err(ContentStoreError::Stale);
        }
        if !frame.is_valid()
            || frame.width != held.metadata.width
            || frame.height != held.metadata.height
            || frame.format != DRM_FORMAT_XRGB8888
            || frame.plane_count != 1
            || usize::try_from(transfer.layout.total_bytes).is_err()
        {
            return Err(ContentStoreError::Malformed);
        }
        let resource = held.description.resource;
        let transfer = self.transfers.remove(&resource).expect("checked capture");
        let bytes = transfer.layout.total_bytes;
        self.usage.staging -= bytes;
        self.usage.reserved_resident -= bytes;
        self.usage.resident += bytes;
        self.response_credits -= 1;
        self.accepted.insert(
            resource,
            Accepted {
                pixels: ContentResourceLease::captured(
                    transfer.description,
                    image.take().expect("checked snapshot"),
                ),
                retiring: false,
                transaction: transfer.transaction,
                release_pending: false,
                capture_evicted: false,
            },
        );
        reservation.take();
        self.status(
            transfer.transaction,
            resource,
            2,
            ContentReason::None,
            bytes,
        );
        Ok(())
    }

    /// Retirement intent, not ResourceReleased, schedules native cache eviction.
    /// The caller runs this on the existing renderer owner. A successful callback
    /// only acknowledges eviction; actual storage uniqueness is checked separately.
    /// Errors leave credit and the eviction obligation intact. No GPU wait occurs.
    pub fn retire_capture_imports<E>(
        &mut self,
        remaining: &mut usize,
        evict: &mut impl FnMut(&Arc<ContentCapturedImage>) -> Result<(), E>,
    ) -> Result<(), E> {
        for value in self.accepted.values_mut() {
            if !(value.retiring || self.revoked) || value.capture_evicted {
                continue;
            }
            let Some(image) = value.pixels.captured_image() else {
                continue;
            };
            image.retire_import_cache();
            if *remaining == 0 {
                break;
            }
            *remaining -= 1;
            evict(image)?;
            value.capture_evicted = true;
        }
        self.collect();
        Ok(())
    }

    pub fn capture_retirement_pending(&self) -> bool {
        self.accepted
            .values()
            .any(|value| (value.retiring || self.revoked) && !value.capture_evicted)
    }

    /// Trusted resource-owner reservation, not DMA-BUF admission. The image
    /// owner must first validate its live allocation, metadata and journal
    /// capacity (ContentEpochRegistry::reserve_capture_image does the first two).
    /// Shares resource IDs, quotas and terminal-record credits with CPU content.
    pub(in crate::shell_content) fn begin_capture(
        &mut self,
        description: ContentResourceBegin,
        metadata: ShellImageMetadata,
        now: u64,
    ) -> Result<ContentCaptureReservation, ContentStoreError> {
        let identity = Arc::new(());
        self.begin_isb(
            TransactionId::from_raw(metadata.offer_id),
            description.clone(),
            now,
        )?;
        self.transfers
            .get_mut(&description.resource)
            .expect("reserved transfer")
            .capture = Some(identity.clone());
        Ok(ContentCaptureReservation {
            description,
            metadata,
            identity,
            allocation: None,
        })
    }

    fn capture_transfer(
        &self,
        reservation: &ContentCaptureReservation,
    ) -> Result<&Transfer, ContentStoreError> {
        if reservation.description.grant != self.limits.grant {
            return Err(ContentStoreError::Stale);
        }
        let transfer = self
            .transfers
            .get(&reservation.description.resource)
            .ok_or(ContentStoreError::Stale)?;
        if !transfer
            .capture
            .as_ref()
            .is_some_and(|id| Arc::ptr_eq(id, &reservation.identity))
        {
            return Err(ContentStoreError::Stale);
        }
        Ok(transfer)
    }

    /// Observe logical withdrawal/expiry without returning any resource credit.
    pub fn capture_stop_reason(
        &mut self,
        reservation: &ContentCaptureReservation,
        now: u64,
    ) -> Result<Option<ContentReason>, ContentStoreError> {
        self.capture_transfer(reservation)?;
        self.expire(now)?;
        Ok(self
            .capture_transfer(reservation)?
            .stopped
            .map(|(reason, _)| reason))
    }

    /// Trusted owner settlement after proven pre-use rollback or proven copy
    /// completion WITHOUT an accepted resource. Never call for timeout alone,
    /// an uncertain native job, or a successful copy awaiting acceptance.
    /// This emits only ResourceStatus, never SourceReleased/ResourceReleased.
    /// An error preserves the caller's ticket and all transfer credit.
    pub fn fail_capture(
        &mut self,
        reservation: &mut Option<ContentCaptureReservation>,
        failure: ContentCaptureFailure,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        let held = reservation.as_ref().ok_or(ContentStoreError::Stale)?;
        self.capture_transfer(held)?;
        self.expire(now)?;
        let transfer = self.capture_transfer(held)?;
        let (reason, transaction) = transfer
            .stopped
            .unwrap_or((failure.reason(), transfer.transaction));
        self.abort_finished(held.description.resource, reason, Some(transaction));
        reservation.take();
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "../../../tests/support/shell_capture_resources.rs"]
mod tests;
