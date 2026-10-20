//! Private real-copy driver for the existing bounded mailbox.
//!
//! Session retains grant/resource correlation outside this driver. Only the
//! real backend is reachable outside this module. Test backends and
//! supplied completions cannot manufacture a public receipt.
use super::executor::{BeginFailure, NextJob};
use super::*;
use sophia_renderer_native_egl::{
    NativeGbmRenderedScanoutContext, NativePendingShellCapture, NativeRendererImageSnapshot,
    NativeShellCaptureOutputFailure, NativeShellCapturePoll, NativeShellCaptureRequest,
    NativeShellCaptureSource, NativeShellCaptureStart,
};

type Output = Result<Arc<NativeRendererImageSnapshot>, NativeShellCaptureOutputFailure>;

/// Before begin, the whole request is returnable. After native submission the
/// native escrow owns it. Only a completion supplies a releasable source again.
#[derive(Debug)]
pub(super) struct Payload {
    pub request: Option<NativeShellCaptureRequest>,
    pub completed_source: Option<NativeShellCaptureSource>,
}

impl Payload {
    pub(super) fn new(request: NativeShellCaptureRequest) -> Self {
        Self {
            request: Some(request),
            completed_source: None,
        }
    }
}

enum Start<P> {
    Pending(P),
    Refused(NativeShellCaptureRequest),
    Uncertain,
}

enum Poll<P> {
    Pending(P),
    Completed {
        source: NativeShellCaptureSource,
        output: Output,
    },
    Uncertain,
}

trait Backend {
    type Pending;
    fn begin(&mut self, request: NativeShellCaptureRequest) -> Start<Self::Pending>;
    fn poll(&mut self, pending: Self::Pending) -> Poll<Self::Pending>;
}

struct Native<'a, T: AsFd>(&'a mut NativeGbmRenderedScanoutContext<T>);

impl<T: AsFd> Backend for Native<'_, T> {
    type Pending = NativePendingShellCapture;

    fn begin(&mut self, request: NativeShellCaptureRequest) -> Start<Self::Pending> {
        match self.0.begin_shell_capture(request) {
            NativeShellCaptureStart::Pending(pending) => Start::Pending(pending),
            NativeShellCaptureStart::Refused { request, .. } => Start::Refused(request),
            NativeShellCaptureStart::Uncertain(_) => Start::Uncertain,
        }
    }

    fn poll(&mut self, pending: Self::Pending) -> Poll<Self::Pending> {
        match self.0.poll_shell_capture(pending) {
            NativeShellCapturePoll::Pending(pending) => Poll::Pending(pending),
            NativeShellCapturePoll::Uncertain(_) => Poll::Uncertain,
            NativeShellCapturePoll::Completed(receipt) => {
                let (source, output) = receipt.into_parts();
                Poll::Completed { source, output }
            }
        }
    }
}

pub(super) type Inbox = CaptureInbox<Payload, Output>;
pub(super) type NativeDriver = Driver<NativePendingShellCapture>;

/// One owner drives this alongside its normal renderer operations. Pending
/// fences require another owner turn; the request fd alone is not a fence fd.
pub(super) struct Driver<P> {
    executor: CaptureExecutor<Payload, Output>,
    pending: std::collections::VecDeque<(JobId, P)>,
    required_budget: Option<Arc<crate::LiveRendererSnapshotBudget>>,
}

struct ServiceTurn<'a, P> {
    driver: &'a mut Driver<P>,
    returned: bool,
}

impl<P> Drop for ServiceTurn<'_, P> {
    fn drop(&mut self) {
        if !self.returned {
            self.driver.close();
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Progress {
    pub operations: usize,
    pub pending_fences: usize,
    pub budget_spent: bool,
}

pub(super) fn native_channel(
    epoch: Arc<LiveRendererSnapshotEpoch>,
) -> io::Result<(Inbox, NativeDriver)> {
    native_owned_channel(epoch, CaptureGraveyard::new(), None)
}

pub(super) fn native_owned_channel(
    epoch: Arc<LiveRendererSnapshotEpoch>,
    graveyard: Arc<CaptureGraveyard<Payload>>,
    required_budget: Option<Arc<crate::LiveRendererSnapshotBudget>>,
) -> io::Result<(Inbox, NativeDriver)> {
    let (inbox, executor) = channel(epoch, graveyard)?;
    Ok((
        inbox,
        Driver {
            executor,
            pending: std::collections::VecDeque::with_capacity(MAX_JOBS),
            required_budget,
        },
    ))
}

impl NativeDriver {
    pub(super) fn service<T: AsFd>(
        &mut self,
        context: &mut NativeGbmRenderedScanoutContext<T>,
        now: Instant,
        budget: usize,
    ) -> io::Result<Progress> {
        self.service_with(&mut Native(context), now, budget)
    }
}

impl<P> Driver<P> {
    // Private fault seam: only service() can reach it outside this module.
    fn service_with<B: Backend<Pending = P>>(
        &mut self,
        backend: &mut B,
        now: Instant,
        budget: usize,
    ) -> io::Result<Progress> {
        let mut turn = ServiceTurn {
            driver: self,
            returned: false,
        };
        let result = turn.driver.service_inner(backend, now, budget);
        turn.returned = result.is_ok();
        result
    }

    fn service_inner<B: Backend<Pending = P>>(
        &mut self,
        backend: &mut B,
        now: Instant,
        budget: usize,
    ) -> io::Result<Progress> {
        let limit = budget.min(MAX_JOBS);
        let mut operations = 0;
        self.executor.observe_cancellations(now);
        // Each previously pending fence gets at most one poll per turn. A
        // pending entry goes to the tail even when the turn budget is smaller.
        let polls = self.pending.len().min(limit);
        for _ in 0..polls {
            let (id, pending) = self.pending.pop_front().expect("bounded pending queue");
            operations += 1;
            match backend.poll(pending) {
                Poll::Pending(pending) => self.pending.push_back((id, pending)),
                Poll::Uncertain => {
                    self.executor.uncertain(id).expect("active native job");
                }
                Poll::Completed { source, output } => {
                    self.executor
                        .active_payload_mut(id)
                        .expect("active native job")
                        .completed_source = Some(source);
                    let proof = ObservedCompletion {
                        id,
                        epoch: self.executor.epoch.clone(),
                    };
                    if self.executor.complete(proof, output, now).is_err() {
                        // Internal mismatch is not permission to reuse another
                        // job. Closing settles every remaining job fail-closed.
                        self.close();
                        return Err(io::Error::other("native completion binding failed"));
                    }
                }
            }
        }
        while operations < limit {
            let id = match self.executor.next(now)? {
                NextJob::Empty => break,
                NextJob::Settled => {
                    operations += 1;
                    continue;
                }
                NextJob::Ready(id) => id,
            };
            operations += 1;
            let epoch = self.executor.epoch.clone();
            let payload = match self.executor.begin(id, now) {
                Ok(payload) => payload,
                Err(BeginFailure::RolledBack(_)) => continue,
                Err(_) => return Err(io::Error::other("native begin binding failed")),
            };
            let request = payload.request.take().expect("unsubmitted request");
            if !Arc::ptr_eq(&request.epoch, &epoch)
                || self
                    .required_budget
                    .as_ref()
                    .is_some_and(|budget| !Arc::ptr_eq(budget, &request.budget))
            {
                payload.request = Some(request);
                self.executor
                    .refused_before_use(id)
                    .expect("active unsubmitted job");
                continue;
            }
            match backend.begin(request) {
                Start::Pending(pending) => self.pending.push_back((id, pending)),
                Start::Uncertain => {
                    self.executor.uncertain(id).expect("active native job");
                }
                Start::Refused(request) => {
                    self.executor
                        .active_payload_mut(id)
                        .expect("active refused job")
                        .request = Some(request);
                    self.executor
                        .refused_before_use(id)
                        .expect("active unsubmitted job");
                }
            }
        }
        Ok(Progress {
            operations,
            pending_fences: self.pending.len(),
            budget_spent: operations == limit,
        })
    }

    pub(super) fn close(&mut self) {
        // Native pending handles quarantine their source/reservation before
        // generic uncertain receipts become observable across the mailbox.
        self.pending.clear();
        self.executor.close();
    }
}

impl<P> AsFd for Driver<P> {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.executor.as_fd()
    }
}

impl<P> Drop for Driver<P> {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
#[path = "../../tests/support/shell_capture_native_driver.rs"]
mod tests;
