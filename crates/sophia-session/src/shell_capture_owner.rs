//! Experimental connection/resource correlation for native capture receipts.
//!
//! Admission remains disabled. Completed receipts can settle resource accounting,
//! but SourceReleased remains a separate journal obligation. Submission requires
//! the runtime owner's logical reservation;
//! every settlement must be reconciled, and the live grant, allocation,
//! topology and resource generation revalidated before publication. Native
//! destination capacity is owned by the request/receipt. This table separately
//! retains logical credit; native completion alone never accepts a resource.
use sophia_protocol::shell_files::{
    SHELL_IMAGE_SOURCE_DMA_BUF, ShellImageMetadata, ShellImagePlane,
};
use sophia_protocol::{ContentGrant, ContentResourceId};
use sophia_renderer_live::{
    LiveShellCaptureCompletion, LiveShellCaptureInbox, LiveShellCaptureJob,
    LiveShellCaptureRequest, LiveShellCaptureSubmitFailure,
};
use sophia_renderer_live::{LiveShellCaptureOutcome, LiveShellCaptureStop};
use sophia_runtime::{
    ContentCaptureFailure, ContentCaptureReservation, ContentEpochRegistry, ContentStoreError,
};
use std::{
    io,
    os::fd::{AsFd, BorrowedFd},
    time::Instant,
};

mod settlement;
pub(crate) use settlement::Publication;

pub(crate) const MAX_JOBS: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Binding {
    pub slot: usize,
    pub grant: ContentGrant,
    pub metadata: ShellImageMetadata,
    pub plane: ShellImagePlane,
}

impl Binding {
    fn resource(&self) -> ContentResourceId {
        ContentResourceId {
            id: self.metadata.resource,
            generation: self.metadata.resource_generation,
        }
    }

    fn matches(&self, request: &LiveShellCaptureRequest) -> bool {
        self.matches_source(&request.source)
    }

    pub(crate) fn conflicts(&self, other: &Self) -> bool {
        self.slot == other.slot
            && self.grant == other.grant
            && (self.metadata.offer_id == other.metadata.offer_id
                || self.resource() == other.resource())
    }

    pub(crate) fn matches_source(&self, s: &sophia_renderer_live::LiveShellCaptureSource) -> bool {
        let m = self.metadata;
        self.grant.connection_epoch != 0
            && self.grant.content_grant_epoch != 0
            && m.admission_epoch == self.grant.content_grant_epoch
            && m.encode().is_ok()
            && m.source_kind == SHELL_IMAGE_SOURCE_DMA_BUF
            && m.source_format == sophia_protocol::DRM_FORMAT_XRGB8888
            && m.plane_count == 1
            && m.descriptor_count == 1
            && m.width == s.width
            && m.height == s.height
            && self.plane.fd_index == 0
            && self.plane.offset == u64::from(s.offset)
            && self.plane.stride == s.stride
            && m.modifier == s.modifier
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SubmitFailure {
    Binding,
    Duplicate,
    Capacity,
    Closed,
    Renderer(LiveShellCaptureSubmitFailure),
}

struct Pending {
    image_id: sophia_renderer_live::LiveRendererImageId,
    job: LiveShellCaptureJob,
    binding: Binding,
    withdrawn: bool,
    reservation: ContentCaptureReservation,
}

#[must_use = "Retain the receipt until both resource and source-release obligations settle"]
pub(crate) struct Settlement {
    image_id: sophia_renderer_live::LiveRendererImageId,
    pub binding: Binding,
    pub receipt: LiveShellCaptureCompletion,
    pub withdrawn: bool,
    pub reservation: Option<ContentCaptureReservation>,
    release: Option<sophia_protocol::shell_files::ShellSourceDisposition>,
}

impl Settlement {
    /// Only an exact completed receipt can enter successful resource accounting.
    /// This does not publish SourceReleased: the completion owner retains this
    /// receipt/source until the image journal can settle that separate obligation.
    pub(crate) fn accept_completed(
        &mut self,
        registry: &mut ContentEpochRegistry,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        let LiveShellCaptureOutcome::Completed {
            output: Ok(snapshot),
            stopped: None,
            ..
        } = self.receipt.outcome()
        else {
            return Err(ContentStoreError::Incomplete);
        };
        accept_proven_snapshot(
            self.binding,
            self.image_id,
            self.withdrawn,
            &mut self.reservation,
            snapshot,
            registry,
            now,
        )
    }

    /// Only a real receipt for pre-use return or completed-but-unpublished copy
    /// can settle logical failure credit. Uncertainty and usable snapshots keep
    /// their ticket. A clock/owner error keeps both ticket and native receipt.
    pub(crate) fn fail_unpublished(
        &mut self,
        registry: &mut ContentEpochRegistry,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        let outcome = self.receipt.outcome();
        reconcile_proven_reads(outcome, || {
            let reservation = self.reservation.as_ref().ok_or(ContentStoreError::Stale)?;
            let stopped = registry.capture_stop_reason(reservation, now)?.is_some();
            let failure = match outcome {
                _ if stopped => ContentCaptureFailure::Withdrawn,
                LiveShellCaptureOutcome::Returned { reason, .. } => failure_for_stop(*reason),
                LiveShellCaptureOutcome::Completed {
                    stopped: Some(reason),
                    ..
                } => failure_for_stop(*reason),
                LiveShellCaptureOutcome::Completed { .. } if self.withdrawn => {
                    ContentCaptureFailure::Withdrawn
                }
                LiveShellCaptureOutcome::Completed { output: Err(_), .. } => {
                    ContentCaptureFailure::Renderer
                }
                LiveShellCaptureOutcome::Completed {
                    output: Ok(snapshot),
                    ..
                } if !snapshot.is_current() => ContentCaptureFailure::Renderer,
                _ => return Err(ContentStoreError::Incomplete),
            };
            registry.fail_capture(&mut self.reservation, failure, now)
        })
    }
}

// Private supplied-storage seam for deterministic owner accounting controls.
// Production reaches it only from the opaque exact-job receipt above.
fn accept_proven_snapshot(
    binding: Binding,
    image_id: sophia_renderer_live::LiveRendererImageId,
    withdrawn: bool,
    reservation: &mut Option<ContentCaptureReservation>,
    snapshot: &std::sync::Arc<sophia_runtime::ContentCapturedImage>,
    registry: &mut ContentEpochRegistry,
    now: u64,
) -> Result<(), ContentStoreError> {
    let held = reservation.as_ref().ok_or(ContentStoreError::Stale)?;
    if withdrawn {
        return Err(ContentStoreError::Revoked);
    }
    if snapshot.image_id().raw() != image_id.raw() {
        return Err(ContentStoreError::Stale);
    }
    if held.description().grant != binding.grant || held.metadata() != &binding.metadata {
        return Err(ContentStoreError::Stale);
    }
    registry.accept_capture_image(reservation, &mut Some(snapshot.clone()), now)
}

// Private supplied-disposition seam; production obtains `outcome` only from
// the opaque native receipt above. This never fabricates a completion receipt.
fn reconcile_proven_reads(
    outcome: &LiveShellCaptureOutcome,
    reconcile: impl FnOnce() -> Result<(), ContentStoreError>,
) -> Result<(), ContentStoreError> {
    if matches!(outcome, LiveShellCaptureOutcome::Uncertain) {
        return Err(ContentStoreError::Incomplete);
    }
    reconcile()
}

fn failure_for_stop(reason: LiveShellCaptureStop) -> ContentCaptureFailure {
    match reason {
        LiveShellCaptureStop::Cancelled => ContentCaptureFailure::Cancelled,
        LiveShellCaptureStop::Deadline => ContentCaptureFailure::Timeout,
        LiveShellCaptureStop::Generation
        | LiveShellCaptureStop::ExecutorClosed
        | LiveShellCaptureStop::RendererRefused => ContentCaptureFailure::Renderer,
    }
}

/// Boxed on the error path: it returns the whole request and reservation.
#[derive(Debug)]
pub(crate) struct Rejected {
    pub reason: SubmitFailure,
    pub request: LiveShellCaptureRequest,
    pub reservation: ContentCaptureReservation,
}

pub(crate) struct Settlements {
    pub completed: Vec<Settlement>,
    pub fault: bool,
}

/// Must outlive connection withdrawal, as the ISB owner does. Withdrawing a
/// connection only cancels interest; its entries remain until exact receipts.
pub(crate) struct Owner {
    inbox: LiveShellCaptureInbox,
    pending: [Option<Pending>; MAX_JOBS],
    settling: [Option<Settlement>; MAX_JOBS],
    closed: bool,
    // Internal identity failure must not silently discard a custody receipt.
    orphaned: Vec<LiveShellCaptureCompletion>,
}

impl Owner {
    pub(crate) fn contains(&self, binding: Binding) -> bool {
        self.pending
            .iter()
            .flatten()
            .map(|p| p.binding)
            .chain(self.settling.iter().flatten().map(|p| p.binding))
            .any(|held| held.conflicts(&binding))
    }

    pub(crate) fn replaceable(&self) -> bool {
        self.closed && self.pending() == 0
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed
    }

    /// Stop publication/submission without dropping source or journal custody.
    pub(crate) fn stop(&mut self) {
        self.closed = true;
        for held in self.pending.iter_mut().flatten() {
            held.withdrawn = true;
            self.inbox.cancel(held.job);
        }
        for held in self.settling.iter_mut().flatten() {
            held.withdrawn = true;
        }
    }

    pub(crate) fn new(inbox: LiveShellCaptureInbox) -> Self {
        Self {
            inbox,
            pending: std::array::from_fn(|_| None),
            settling: std::array::from_fn(|_| None),
            closed: false,
            orphaned: Vec::new(),
        }
    }

    pub(crate) fn submit(
        &mut self,
        binding: Binding,
        request: LiveShellCaptureRequest,
        reservation: ContentCaptureReservation,
        deadline: Instant,
    ) -> Result<LiveShellCaptureJob, Box<Rejected>> {
        if self.closed {
            return Err(Box::new(Rejected {
                reason: SubmitFailure::Closed,
                request,
                reservation,
            }));
        }
        if !binding.matches(&request)
            || reservation.description().grant != binding.grant
            || *reservation.metadata() != binding.metadata
        {
            return Err(Box::new(Rejected {
                reason: SubmitFailure::Binding,
                request,
                reservation,
            }));
        }
        // Both offer and resource identity are connection-scoped. A replacement
        // connection may use the same numbers while the old copy still settles.
        if self
            .pending
            .iter()
            .flatten()
            .map(|p| p.binding)
            .chain(self.settling.iter().flatten().map(|p| p.binding))
            .any(|held| {
                held.slot == binding.slot
                    && held.grant == binding.grant
                    && (held.metadata.offer_id == binding.metadata.offer_id
                        || held.resource() == binding.resource())
            })
        {
            return Err(Box::new(Rejected {
                reason: SubmitFailure::Duplicate,
                request,
                reservation,
            }));
        }
        let Some(slot) = self
            .pending
            .iter()
            .position(Option::is_none)
            .filter(|_| self.pending() < MAX_JOBS)
        else {
            return Err(Box::new(Rejected {
                reason: SubmitFailure::Capacity,
                request,
                reservation,
            }));
        };
        let image_id = request.image_id;
        let job = match self.inbox.submit(request, deadline) {
            Ok(job) => job,
            Err((error, request)) => {
                return Err(Box::new(Rejected {
                    reason: SubmitFailure::Renderer(error),
                    request,
                    reservation,
                }));
            }
        };
        self.pending[slot] = Some(Pending {
            image_id,
            job,
            binding,
            withdrawn: false,
            reservation,
        });
        Ok(job)
    }

    pub(crate) fn withdraw(&mut self, slot: usize, grant: ContentGrant) {
        for settlement in self.settling.iter_mut().flatten() {
            if settlement.binding.slot == slot && settlement.binding.grant == grant {
                settlement.withdrawn = true;
            }
        }
        for p in self.pending.iter_mut().flatten() {
            if p.binding.slot == slot && p.binding.grant == grant {
                p.withdrawn = true;
                self.inbox.cancel(p.job);
            }
        }
    }

    /// Resource expiry/cancellation never discards the pending native job. Keep
    /// visiting every ticket even when one owner's clock or identity is wrong.
    pub(crate) fn observe_resources(
        &mut self,
        registry: &mut ContentEpochRegistry,
        now: u64,
    ) -> Result<(), ContentStoreError> {
        let mut error = None;
        self.observe_resource_failures(registry, now, |_, e| {
            error.get_or_insert(e);
        });
        error.map_or(Ok(()), Err)
    }

    fn observe_resource_failures(
        &mut self,
        registry: &mut ContentEpochRegistry,
        now: u64,
        mut report: impl FnMut(Binding, ContentStoreError),
    ) {
        for p in self.pending.iter_mut().flatten() {
            match registry.capture_stop_reason(&p.reservation, now) {
                Ok(None) => (),
                Ok(Some(_)) => {
                    p.withdrawn = true;
                    self.inbox.cancel(p.job);
                }
                Err(e) => {
                    if e != ContentStoreError::ClockRegression {
                        p.withdrawn = true;
                        self.inbox.cancel(p.job);
                    }
                    report(p.binding, e);
                }
            }
        }
    }

    pub(crate) fn cancel_resource(
        &mut self,
        slot: usize,
        grant: ContentGrant,
        resource: ContentResourceId,
    ) {
        for p in self.pending.iter_mut().flatten() {
            if p.binding.slot == slot
                && p.binding.grant == grant
                && p.binding.resource() == resource
            {
                p.withdrawn = true;
                self.inbox.cancel(p.job);
            }
        }
    }

    pub(crate) fn take_completed(&mut self, now: Instant) -> io::Result<Settlements> {
        let batch = self.inbox.take_completed(now)?;
        let mut completed = Vec::with_capacity(MAX_JOBS);
        let mut fault = batch.fault;
        for receipt in batch.receipts {
            if let Some(slot) = self
                .pending
                .iter()
                .position(|p| p.as_ref().is_some_and(|p| p.job == receipt.job()))
            {
                let p = self.pending[slot].take().expect("matched pending capture");
                completed.push(Settlement {
                    image_id: p.image_id,
                    binding: p.binding,
                    receipt,
                    withdrawn: p.withdrawn,
                    reservation: Some(p.reservation),
                    release: None,
                });
            } else {
                self.orphaned.push(receipt);
                fault = true;
            }
        }
        if fault {
            self.closed = true;
            for p in self.pending.iter().flatten() {
                self.inbox.cancel(p.job);
            }
        }
        Ok(Settlements { completed, fault })
    }
}

impl AsFd for Owner {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.inbox.as_fd()
    }
}

#[cfg(test)]
#[path = "../tests/support/shell_capture_owner.rs"]
mod tests;
