//! Deterministic mailbox/native-adapter composition with a private backend.
//! This supplies completion, never runs EGL, and deliberately supplies an output
//! failure after completion: it proves source custody, not successful rendering.
use super::*;
use crate::LiveRendererSnapshotBudget;
use sophia_renderer_native_egl::{NativeGbmScanoutBufferExportDetail, NativeRendererImageId};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::Duration;

struct Pending {
    request: Option<NativeShellCaptureRequest>,
    parked: Arc<Mutex<Vec<NativeShellCaptureRequest>>>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(request) = self.request.take() {
            self.parked
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(request);
        }
    }
}

#[derive(Default)]
struct Fake {
    ready: bool,
    refuse: bool,
    uncertain: bool,
    panic_on_poll: bool,
    starts: Vec<u64>,
    polls: Vec<u64>,
    parked: Arc<Mutex<Vec<NativeShellCaptureRequest>>>,
}

impl Backend for Fake {
    type Pending = Pending;
    fn begin(&mut self, request: NativeShellCaptureRequest) -> Start<Pending> {
        self.starts.push(request.image_id.raw());
        if self.refuse {
            return Start::Refused(request);
        }
        if self.uncertain {
            self.parked
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(request);
            return Start::Uncertain;
        }
        Start::Pending(Pending {
            request: Some(request),
            parked: self.parked.clone(),
        })
    }
    fn poll(&mut self, mut pending: Pending) -> Poll<Pending> {
        assert!(!self.panic_on_poll, "simulated native panic");
        self.polls
            .push(pending.request.as_ref().unwrap().image_id.raw());
        if self.uncertain {
            return Poll::Uncertain;
        }
        if !self.ready {
            return Poll::Pending(pending);
        }
        let r = pending.request.take().unwrap();
        Poll::Completed {
            source: r.source,
            output: Err(NativeShellCaptureOutputFailure {
                detail: NativeGbmScanoutBufferExportDetail::InvalidTarget,
                reservation: r.reservation,
            }),
        }
    }
}

struct Fixture {
    inbox: Inbox,
    driver: Driver<Pending>,
    backend: Fake,
    epoch: Arc<LiveRendererSnapshotEpoch>,
    budget: Arc<LiveRendererSnapshotBudget>,
    now: Instant,
}

impl Fixture {
    fn new() -> Self {
        let epoch = LiveRendererSnapshotEpoch::new(1, 2, 3);
        let (inbox, executor) = channel(epoch.clone(), CaptureGraveyard::new()).unwrap();
        Self {
            inbox,
            driver: Driver {
                executor,
                pending: Default::default(),
            },
            backend: Fake::default(),
            epoch,
            budget: LiveRendererSnapshotBudget::new(8, 4096),
            now: Instant::now(),
        }
    }
    fn request(&self, image: u64) -> (Request<Payload>, i32) {
        let (source, _observer) = UnixStream::pair().unwrap();
        let fd = source.as_raw_fd();
        let request = NativeShellCaptureRequest {
            source: NativeShellCaptureSource {
                fd: source.into(),
                width: 4,
                height: 4,
                modifier: 0,
                offset: 0,
                stride: 16,
            },
            image_id: NativeRendererImageId::from_raw(image),
            budget: self.budget.clone(),
            reservation: self.budget.reserve_capture(64).unwrap(),
            epoch: self.epoch.clone(),
        };
        (
            Request {
                payload: Payload::new(request),
                deadline: self.now + Duration::from_secs(60),
            },
            fd,
        )
    }
    fn submit(&mut self, image: u64) -> (JobId, i32) {
        let (request, fd) = self.request(image);
        (self.inbox.submit(request).unwrap(), fd)
    }
    fn service(&mut self, budget: usize) -> Progress {
        self.driver
            .service_with(&mut self.backend, self.now, budget)
            .unwrap()
    }
}

#[test]
fn pending_never_releases_then_exact_job_completion_returns_source_and_capacity() {
    let mut f = Fixture::new();
    let (id, fd) = f.submit(1);
    assert_eq!(f.service(0).operations, 0);
    assert!(f.backend.starts.is_empty());
    assert_eq!(f.service(1).pending_fences, 1);
    assert!(f.backend.polls.is_empty());
    assert_eq!(f.service(4).operations, 1);
    assert_eq!(f.backend.polls, [1]);
    assert!(
        f.inbox
            .take_completed(f.now)
            .unwrap()
            .completions
            .is_empty()
    );
    assert_eq!(f.budget.usage(), (1, 64));
    f.backend.ready = true;
    assert_eq!(f.service(1).pending_fences, 0);
    let batch = f.inbox.take_completed(f.now).unwrap();
    assert!(!batch.fault);
    assert_eq!(batch.completions.len(), 1);
    assert_eq!(batch.completions[0].id, id);
    let Outcome::Completed { payload, output } = &batch.completions[0].outcome else {
        panic!("completed")
    };
    assert!(payload.request.is_none());
    assert_eq!(
        payload.completed_source.as_ref().unwrap().fd.as_raw_fd(),
        fd
    );
    assert!(output.is_err()); // Copy completed; snapshot export failed in this fixture.
    drop(batch);
    assert_eq!(f.budget.usage(), (0, 0));
    assert!(
        f.inbox
            .take_completed(f.now)
            .unwrap()
            .completions
            .is_empty()
    );
}

#[test]
fn cancellation_before_begin_rolls_back_without_entering_native_backend() {
    let mut f = Fixture::new();
    let (id, _) = f.submit(1);
    f.inbox.cancel(id);
    f.service(4);
    assert!(f.backend.starts.is_empty());
    let batch = f.inbox.take_completed(f.now).unwrap();
    assert!(
        matches!(&batch.completions[0].outcome, Outcome::RolledBack { reason: StopReason::Cancelled, payload } if payload.request.is_some())
    );
}

#[test]
fn cancellation_after_begin_still_polls_and_discards_completed_publication() {
    let mut f = Fixture::new();
    let (id, fd) = f.submit(1);
    f.service(1);
    f.inbox.cancel(id);
    f.backend.ready = true;
    f.service(1);
    let batch = f.inbox.take_completed(f.now).unwrap();
    assert!(
        matches!(&batch.completions[0].outcome, Outcome::CompletedDiscarded { reason: StopReason::Cancelled, payload, .. } if payload.completed_source.as_ref().unwrap().fd.as_raw_fd() == fd)
    );
}

#[test]
fn epoch_withdrawal_does_not_replace_copy_completion_or_allow_publication() {
    let mut f = Fixture::new();
    f.submit(1);
    f.service(1);
    f.epoch.invalidate();
    f.service(1);
    assert!(
        f.inbox
            .take_completed(f.now)
            .unwrap()
            .completions
            .is_empty()
    );
    assert_eq!(f.budget.usage(), (1, 64));
    f.backend.ready = true;
    f.service(1);
    let batch = f.inbox.take_completed(f.now).unwrap();
    assert!(
        matches!(&batch.completions[0].outcome, Outcome::CompletedDiscarded { reason: StopReason::Generation, payload, .. } if payload.completed_source.is_some())
    );
}

#[test]
fn inbox_is_cross_owner_but_native_work_stays_in_the_renderer_driver() {
    fn send<T: Send>() {}
    send::<Inbox>();
}

#[test]
fn native_refusal_returns_the_original_request_and_reservation() {
    let mut f = Fixture::new();
    let (_, fd) = f.submit(1);
    f.backend.refuse = true;
    f.service(1);
    let batch = f.inbox.take_completed(f.now).unwrap();
    assert!(
        matches!(&batch.completions[0].outcome, Outcome::RolledBack { reason: StopReason::RendererRefused, payload } if payload.request.as_ref().unwrap().source.fd.as_raw_fd() == fd)
    );
    assert_eq!(f.budget.usage(), (1, 64));
    drop(batch);
    assert_eq!(f.budget.usage(), (0, 0));
}

#[test]
fn equal_epoch_numbers_do_not_bind_a_different_snapshot_epoch() {
    let mut f = Fixture::new();
    let (mut request, _) = f.request(1);
    request.payload.request.as_mut().unwrap().epoch = LiveRendererSnapshotEpoch::new(1, 2, 3);
    f.inbox.submit(request).unwrap();
    f.service(1);
    assert!(f.backend.starts.is_empty());
    let batch = f.inbox.take_completed(f.now).unwrap();
    assert!(matches!(
        batch.completions[0].outcome,
        Outcome::RolledBack {
            reason: StopReason::RendererRefused,
            ..
        }
    ));
}

#[test]
fn uncertain_native_result_never_returns_source_or_releases_reservation() {
    let mut f = Fixture::new();
    f.submit(1);
    f.service(1);
    f.backend.uncertain = true;
    f.service(1);
    let batch = f.inbox.take_completed(f.now).unwrap();
    assert!(matches!(
        batch.completions[0].outcome,
        Outcome::FailedUncertain
    ));
    assert_eq!(f.backend.parked.lock().unwrap().len(), 1);
    assert_eq!(f.budget.usage(), (1, 64));
    assert!(!f.epoch.is_valid());
    drop(batch);
    assert_eq!(f.budget.usage(), (1, 64));
}

#[test]
fn owner_shutdown_parks_native_handles_before_publishing_uncertainty() {
    let mut f = Fixture::new();
    f.submit(1);
    f.service(1);
    f.driver.close();
    assert_eq!(f.backend.parked.lock().unwrap().len(), 1);
    let batch = f.inbox.take_completed(f.now).unwrap();
    assert!(!batch.fault);
    assert!(matches!(
        batch.completions[0].outcome,
        Outcome::FailedUncertain
    ));
    assert_eq!(f.budget.usage(), (1, 64));
}

#[test]
fn unwinding_service_settles_every_job_even_if_the_driver_survives_catch() {
    let mut f = Fixture::new();
    f.submit(1);
    f.submit(2);
    f.service(4);
    f.backend.panic_on_poll = true;
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f.service(1)));
    assert!(panic.is_err());
    assert_eq!(f.backend.parked.lock().unwrap().len(), 2);
    let batch = f.inbox.take_completed(f.now).unwrap();
    assert!(!batch.fault);
    assert_eq!(batch.completions.len(), 2);
    assert!(
        batch
            .completions
            .iter()
            .all(|c| matches!(c.outcome, Outcome::FailedUncertain))
    );
    assert!(!f.epoch.is_valid());
}

#[test]
fn small_turn_budget_rotates_fences_and_preserves_each_job_binding() {
    let mut f = Fixture::new();
    let (first, first_fd) = f.submit(1);
    let (second, second_fd) = f.submit(2);
    assert_eq!(f.service(4).operations, 2);
    for _ in 0..3 {
        assert_eq!(f.service(1).operations, 1);
    }
    assert_eq!(f.backend.polls, [1, 2, 1]);
    f.backend.ready = true;
    assert_eq!(f.service(4).operations, 2);
    let batch = f.inbox.take_completed(f.now).unwrap();
    assert_eq!(batch.completions.len(), 2);
    for c in batch.completions {
        let Outcome::Completed { payload, .. } = c.outcome else {
            panic!("completed")
        };
        let expected = if c.id == first {
            first_fd
        } else {
            assert_eq!(c.id, second);
            second_fd
        };
        assert_eq!(payload.completed_source.unwrap().fd.as_raw_fd(), expected);
    }
}
