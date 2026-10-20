//! Supplied-completion controls of the public facade; no EGL work.
use super::*;
use crate::LiveRendererSnapshotBudget;
use std::{
    os::{fd::AsRawFd, unix::net::UnixStream},
    time::Duration,
};

struct Fixture {
    inbox: LiveShellCaptureInbox,
    executor: CaptureExecutor<
        native::Payload,
        Result<
            Arc<sophia_renderer_native_egl::NativeRendererImageSnapshot>,
            LiveShellCaptureOutputFailure,
        >,
    >,
    epoch: Arc<LiveRendererSnapshotEpoch>,
    budget: Arc<LiveRendererSnapshotBudget>,
    now: Instant,
}

impl Fixture {
    fn new() -> Self {
        let epoch = LiveRendererSnapshotEpoch::new(1, 2, 3);
        let (inbox, executor) = channel(epoch.clone(), CaptureGraveyard::new()).unwrap();
        Self {
            inbox: LiveShellCaptureInbox(inbox),
            executor,
            epoch,
            budget: LiveRendererSnapshotBudget::new(8, 512),
            now: Instant::now(),
        }
    }
    fn request(&self) -> LiveShellCaptureRequest {
        let (fd, _) = UnixStream::pair().unwrap();
        LiveShellCaptureRequest {
            source: LiveShellCaptureSource {
                fd: fd.into(),
                width: 4,
                height: 4,
                stride: 16,
                modifier: 0,
                offset: 0,
            },
            image_id: crate::LiveRendererImageId::from_raw(1),
            budget: self.budget.clone(),
            reservation: self.budget.reserve_capture(64).unwrap(),
            epoch: self.epoch.clone(),
        }
    }
    fn submit(&mut self) -> (LiveShellCaptureJob, i32) {
        let request = self.request();
        let fd = request.source.fd.as_raw_fd();
        (
            self.inbox
                .submit(request, self.now + Duration::from_secs(10))
                .unwrap(),
            fd,
        )
    }
    fn begin(
        &mut self,
        job: LiveShellCaptureJob,
    ) -> sophia_renderer_native_egl::NativeShellCaptureRequest {
        assert!(
            matches!(self.executor.next(self.now).unwrap(), executor::NextJob::Ready(id) if id == job.0)
        );
        self.executor
            .begin(job.0, self.now)
            .unwrap()
            .request
            .take()
            .unwrap()
    }
    fn finish(
        &mut self,
        job: LiveShellCaptureJob,
        request: sophia_renderer_native_egl::NativeShellCaptureRequest,
    ) {
        self.executor
            .active_payload_mut(job.0)
            .unwrap()
            .completed_source = Some(request.source);
        self.executor.complete(ObservedCompletion { id: job.0, epoch: self.epoch.clone() }, Err(LiveShellCaptureOutputFailure {
            detail: sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::InvalidTarget,
            reservation: request.reservation,
        }), self.now).unwrap();
    }
}

#[test]
fn full_submission_returns_exact_source_and_reservation() {
    let mut f = Fixture::new();
    for _ in 0..4 {
        f.submit();
    }
    let request = f.request();
    let fd = request.source.fd.as_raw_fd();
    let (reason, request) = f.inbox.submit(request, f.now).unwrap_err();
    assert_eq!(reason, LiveShellCaptureSubmitFailure::Capacity);
    assert_eq!(request.source.fd.as_raw_fd(), fd);
    assert_eq!(f.budget.usage(), (5, 320));
    drop(request);
    assert_eq!(f.budget.usage(), (4, 256));
}

#[test]
fn completed_copy_returns_one_exact_receipt_even_when_output_failed() {
    let mut f = Fixture::new();
    let (job, fd) = f.submit();
    let request = f.begin(job);
    assert!(f.inbox.take_completed(f.now).unwrap().receipts.is_empty());
    f.finish(job, request);
    let mut batch = f.inbox.take_completed(f.now).unwrap();
    assert!(!batch.fault);
    assert_eq!(batch.receipts.len(), 1);
    let receipt = batch.receipts.pop().unwrap();
    assert_eq!(receipt.job(), job);
    let LiveShellCaptureOutcome::Completed {
        source,
        output,
        stopped,
    } = receipt.into_outcome()
    else {
        panic!("completion")
    };
    assert_eq!(source.fd.as_raw_fd(), fd);
    assert_eq!(stopped, None);
    assert!(output.is_err());
    assert_eq!(f.budget.usage(), (1, 64));
    drop(output);
    assert_eq!(f.budget.usage(), (0, 0));
    assert!(f.inbox.take_completed(f.now).unwrap().receipts.is_empty());
}

#[test]
fn withdrawal_after_completion_before_take_forbids_publication() {
    for expiry in [false, true] {
        let mut f = Fixture::new();
        let (job, _) = f.submit();
        let request = f.begin(job);
        f.finish(job, request);
        if !expiry {
            assert!(f.inbox.cancel(job));
        }
        let now = if expiry {
            f.now + Duration::from_secs(11)
        } else {
            f.now
        };
        let receipt = f.inbox.take_completed(now).unwrap().receipts.pop().unwrap();
        let LiveShellCaptureOutcome::Completed { stopped, .. } = receipt.into_outcome() else {
            panic!("completion")
        };
        assert_eq!(
            stopped,
            Some(if expiry {
                LiveShellCaptureStop::Deadline
            } else {
                LiveShellCaptureStop::Cancelled
            })
        );
    }
}

#[test]
fn executor_close_before_begin_returns_request_instead_of_completion() {
    let (mut inbox, executor) =
        live_shell_capture_channel(LiveRendererSnapshotEpoch::new(1, 2, 3)).unwrap();
    // A different epoch is rejected at begin, but close occurs before any begin.
    let f = Fixture::new();
    let request = f.request();
    let fd = request.source.fd.as_raw_fd();
    let job = inbox
        .submit(request, f.now + Duration::from_secs(10))
        .unwrap();
    drop(executor);
    let receipt = inbox.take_completed(f.now).unwrap().receipts.pop().unwrap();
    assert_eq!(receipt.job(), job);
    let LiveShellCaptureOutcome::Returned { request, reason } = receipt.into_outcome() else {
        panic!("rollback")
    };
    assert_eq!(reason, LiveShellCaptureStop::ExecutorClosed);
    assert_eq!(request.source.fd.as_raw_fd(), fd);
}

#[test]
fn uncertainty_never_returns_source_or_reservation() {
    let mut f = Fixture::new();
    let (job, _) = f.submit();
    let _request = f.begin(job);
    // The native layer retains this request in real use. This supplied seam
    // checks only that the public uncertainty receipt cannot return it.
    f.executor.uncertain(job.0).unwrap();
    let receipt = f
        .inbox
        .take_completed(f.now)
        .unwrap()
        .receipts
        .pop()
        .unwrap();
    assert!(matches!(
        receipt.into_outcome(),
        LiveShellCaptureOutcome::Uncertain
    ));
}

#[test]
fn inbox_crosses_owners() {
    fn send<T: Send>() {}
    send::<LiveShellCaptureInbox>();
}
