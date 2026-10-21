//! Supplied-completion controls for the private custody state machine.
//! Sockets stand in for source planes; no EGL, DMA-BUF import or GPU proof.
use super::executor::{BeginFailure, NextJob, ProofFailure};
use super::*;
use crate::{LiveRendererSnapshotBudget, LiveRendererSnapshotReservation};
use std::io::Read;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::Duration;

#[derive(Debug)]
struct Payload {
    source: OwnedFd,
    reservation: LiveRendererSnapshotReservation,
}

type Inbox = CaptureInbox<Payload, u64>;
type Executor = CaptureExecutor<Payload, u64>;

fn fixture() -> (
    Inbox,
    Executor,
    Arc<CaptureGraveyard<Payload>>,
    Arc<LiveRendererSnapshotBudget>,
    Arc<LiveRendererSnapshotEpoch>,
) {
    let epoch = LiveRendererSnapshotEpoch::new(1, 2, 3);
    let graveyard = CaptureGraveyard::new();
    let budget = LiveRendererSnapshotBudget::new(8, 512);
    let (inbox, executor) = channel(epoch.clone(), graveyard.clone()).unwrap();
    (inbox, executor, graveyard, budget, epoch)
}

fn make_request(
    budget: &Arc<LiveRendererSnapshotBudget>,
    now: Instant,
) -> (Request<Payload>, UnixStream) {
    let (source, peer) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    (
        Request {
            payload: Payload {
                source: source.into(),
                reservation: budget.reserve_capture(64).unwrap(),
            },
            deadline: now + Duration::from_secs(60),
        },
        peer,
    )
}

// This is the only completion supplier. Real GPU admission cannot call it.
fn observed(id: JobId, epoch: &Arc<LiveRendererSnapshotEpoch>) -> ObservedCompletion {
    ObservedCompletion {
        id,
        epoch: epoch.clone(),
    }
}

fn ready(fd: &impl AsFd) -> bool {
    let mut fds = [rustix::event::PollFd::new(fd, rustix::event::PollFlags::IN)];
    rustix::event::poll(
        &mut fds,
        Some(&rustix::event::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        }),
    )
    .unwrap()
        == 1
}

fn held(peer: &mut UnixStream) {
    assert_eq!(
        peer.read(&mut [0]).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

fn released(peer: &mut UnixStream) {
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);
}

#[test]
fn full_and_untaken_completion_limits_return_the_exact_request() {
    let (mut inbox, mut executor, graveyard, budget, epoch) = fixture();
    let now = Instant::now();
    let mut peers = Vec::new();
    for output in 0..4 {
        let (request, peer) = make_request(&budget, now);
        peers.push(peer);
        let id = inbox.submit(request).unwrap();
        assert_eq!(executor.next(now).unwrap(), NextJob::Ready(id));
        executor.begin(id, now).unwrap();
        executor
            .complete(observed(id, &epoch), output, now)
            .unwrap();
    }
    assert!(ready(&inbox));
    assert_eq!(graveyard.usage(), (4, 0));
    let (request, mut peer) = make_request(&budget, now);
    let fd = request.payload.source.as_raw_fd();
    let deadline = request.deadline;
    let (reason, request) = inbox.submit(request).unwrap_err();
    assert_eq!(reason, SubmitFailure::Capacity);
    assert_eq!(request.payload.source.as_raw_fd(), fd);
    assert_eq!(request.deadline, deadline);
    assert_eq!(request.payload.reservation.byte_bound(), 64);
    held(&mut peer);
    let batch = inbox.take_completed(now).unwrap();
    assert!(!batch.fault);
    assert_eq!(batch.completions.len(), 4);
    assert_eq!(graveyard.usage(), (0, 0));
    assert_eq!(budget.usage(), (5, 320)); // Returned receipts still own storage.
    for peer in &mut peers {
        held(peer);
    }
    drop(batch);
    for peer in &mut peers {
        released(peer);
    }
    let id = inbox.submit(request).unwrap();
    assert!(ready(&executor)); // A publication after the clear wakes again.
    executor.close();
    let batch = inbox.take_completed(now).unwrap();
    assert!(!batch.fault);
    assert_eq!(batch.completions[0].id, id);
    assert!(matches!(
        batch.completions[0].outcome,
        Outcome::RolledBack { .. }
    ));
    drop(batch);
    released(&mut peer);
    assert_eq!(budget.usage(), (0, 0));
}

#[test]
fn queued_cancellation_and_deadline_are_bounded_pre_use_rollbacks() {
    let (mut inbox, mut executor, graveyard, budget, _) = fixture();
    let now = Instant::now();
    let (one, mut first) = make_request(&budget, now);
    let (mut two, mut second) = make_request(&budget, now);
    two.deadline = now;
    let id = inbox.submit(one).unwrap();
    inbox.submit(two).unwrap();
    assert!(inbox.cancel(id));
    assert_eq!(executor.next(now).unwrap(), NextJob::Settled);
    assert!(ready(&executor)); // The unconsumed request remains runnable.
    let batch = inbox.take_completed(now).unwrap();
    assert_eq!(batch.completions.len(), 1);
    assert!(matches!(
        batch.completions[0].outcome,
        Outcome::RolledBack {
            reason: StopReason::Cancelled,
            ..
        }
    ));
    held(&mut first);
    drop(batch);
    released(&mut first);
    held(&mut second);
    assert_eq!(executor.next(now).unwrap(), NextJob::Settled);
    let batch = inbox.take_completed(now).unwrap();
    assert!(matches!(
        batch.completions[0].outcome,
        Outcome::RolledBack {
            reason: StopReason::Deadline,
            ..
        }
    ));
    drop(batch);
    released(&mut second);
    assert_eq!(executor.next(now).unwrap(), NextJob::Empty);
    assert!(!ready(&executor));
    assert_eq!(graveyard.usage(), (0, 0));
    assert_eq!(budget.usage(), (0, 0));
}

#[test]
fn cancellation_and_expiry_never_substitute_for_completion() {
    for mode in ["cancel", "deadline", "queued_receipt_cancel", "generation"] {
        let (mut inbox, mut executor, _, budget, epoch) = fixture();
        let now = Instant::now();
        let (request, mut peer) = make_request(&budget, now);
        let deadline = request.deadline;
        let id = inbox.submit(request).unwrap();
        executor.next(now).unwrap();
        executor.begin(id, now).unwrap();
        match mode {
            "cancel" => {
                inbox.cancel(id);
            }
            "deadline" => executor.observe_cancellations(deadline),
            "generation" => epoch.invalidate(),
            _ => {}
        }
        assert!(inbox.take_completed(now).unwrap().completions.is_empty());
        assert_eq!(budget.usage(), (1, 64));
        held(&mut peer);
        executor.complete(observed(id, &epoch), 9, now).unwrap();
        if mode == "queued_receipt_cancel" {
            inbox.cancel(id);
        }
        let batch = inbox.take_completed(now).unwrap();
        assert!(!batch.fault);
        assert!(
            matches!(
                batch.completions[0].outcome,
                Outcome::CompletedDiscarded { output: 9, .. }
            ),
            "{mode}"
        );
        held(&mut peer);
        drop(batch);
        released(&mut peer);
        assert_eq!(budget.usage(), (0, 0));
    }
}

mod faults {
    use super::*;
    include!("shell_capture_faults.rs");
}
