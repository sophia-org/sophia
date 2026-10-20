//! Experimental connection/resource correlation for native capture receipts.
//!
//! This is not admission and does not create a ContentResourceLease or emit a
//! journal event. The future resource owner must reserve logical shell capacity
//! before submit, reconcile every settlement, and revalidate the live grant,
//! allocation, topology and resource generation before publication. Native
//! destination capacity is owned by the request/receipt, never by this table.
use sophia_protocol::shell_files::{
    SHELL_IMAGE_SOURCE_DMA_BUF, ShellImageMetadata, ShellImagePlane,
};
use sophia_protocol::{ContentGrant, ContentResourceId};
use sophia_renderer_live::{
    LiveShellCaptureCompletion, LiveShellCaptureInbox, LiveShellCaptureJob,
    LiveShellCaptureRequest, LiveShellCaptureSubmitFailure,
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
}

pub(crate) struct Settlement {
    pub binding: Binding,
    pub receipt: LiveShellCaptureCompletion,
    pub withdrawn: bool,
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
        deadline: Instant,
    ) -> Result<LiveShellCaptureJob, (SubmitFailure, LiveShellCaptureRequest)> {
        if self.closed {
            return Err((SubmitFailure::Closed, request));
        }
        if !binding.matches(&request) {
            return Err((SubmitFailure::Binding, request));
        }
        // Both offer and resource identity are connection-scoped. A replacement
        // connection may use the same numbers while the old copy still settles.
        if self.pending.iter().flatten().any(|p| {
            p.binding.slot == binding.slot
                && p.binding.grant == binding.grant
                && (p.binding.metadata.offer_id == binding.metadata.offer_id
                    || p.binding.resource() == binding.resource())
        }) {
            return Err((SubmitFailure::Duplicate, request));
        }
        let Some(slot) = self.pending.iter().position(Option::is_none) else {
            return Err((SubmitFailure::Capacity, request));
        };
        let job = self
            .inbox
            .submit(request, deadline)
            .map_err(|(error, request)| (SubmitFailure::Renderer(error), request))?;
        self.pending[slot] = Some(Pending {
            job,
            binding,
            withdrawn: false,
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
