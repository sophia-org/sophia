//! Logical resource credit, independent of physical renderer allocation credit.
//! No renderer type, source FD, completion proof or publication API lives here.
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
}

impl ContentCaptureReservation {
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
