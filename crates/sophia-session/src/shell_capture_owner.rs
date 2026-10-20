//! Experimental connection/resource correlation for native capture receipts.
//!
//! This is not admission and does not create a ContentResourceLease or emit a
//! journal event. Submission requires the runtime owner's logical reservation;
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

const MAX_JOBS: usize = 4;

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
        let m = self.metadata;
        let s = &request.source;
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
    job: LiveShellCaptureJob,
    binding: Binding,
    withdrawn: bool,
    reservation: ContentCaptureReservation,
}

pub(crate) struct Settlement {
    pub binding: Binding,
    pub receipt: LiveShellCaptureCompletion,
    pub withdrawn: bool,
    pub reservation: Option<ContentCaptureReservation>,
}

impl Settlement {
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
    closed: bool,
    // Internal identity failure must not silently discard a custody receipt.
    orphaned: Vec<LiveShellCaptureCompletion>,
}

impl Owner {
    pub(crate) fn new(inbox: LiveShellCaptureInbox) -> Self {
        Self {
            inbox,
            pending: std::array::from_fn(|_| None),
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
    ) -> Result<LiveShellCaptureJob, Rejected> {
        if self.closed {
            return Err(Rejected {
                reason: SubmitFailure::Closed,
                request,
                reservation,
            });
        }
        if !binding.matches(&request)
            || reservation.description().grant != binding.grant
            || *reservation.metadata() != binding.metadata
        {
            return Err(Rejected {
                reason: SubmitFailure::Binding,
                request,
                reservation,
            });
        }
        // Both offer and resource identity are connection-scoped. A replacement
        // connection may use the same numbers while the old copy still settles.
        if self.pending.iter().flatten().any(|p| {
            p.binding.slot == binding.slot
                && p.binding.grant == binding.grant
                && (p.binding.metadata.offer_id == binding.metadata.offer_id
                    || p.binding.resource() == binding.resource())
        }) {
            return Err(Rejected {
                reason: SubmitFailure::Duplicate,
                request,
                reservation,
            });
        }
        let Some(slot) = self.pending.iter().position(Option::is_none) else {
            return Err(Rejected {
                reason: SubmitFailure::Capacity,
                request,
                reservation,
            });
        };
        let job = match self.inbox.submit(request, deadline) {
            Ok(job) => job,
            Err((error, request)) => {
                return Err(Rejected {
                    reason: SubmitFailure::Renderer(error),
                    request,
                    reservation,
                });
            }
        };
        self.pending[slot] = Some(Pending {
            job,
            binding,
            withdrawn: false,
            reservation,
        });
        Ok(job)
    }

    pub(crate) fn withdraw(&mut self, slot: usize, grant: ContentGrant) {
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
                    error.get_or_insert(e);
                }
            }
        }
        error.map_or(Ok(()), Err)
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
                    binding: p.binding,
                    receipt,
                    withdrawn: p.withdrawn,
                    reservation: Some(p.reservation),
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
