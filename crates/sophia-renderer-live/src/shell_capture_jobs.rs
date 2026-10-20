//! Private custody handoff and native adapter, not an admission API.
//!
//! P owns source descriptors and their reservation; O is the eventual renderer
//! output. The native adapter must mark submission BEFORE import or any GPU use.
//! Only that adapter may construct an ObservedCompletion after the exact job's
//! fence signals. The native adapter owns the real capture handle; supplied
//! completions remain confined to private test children.
use crate::LiveRendererSnapshotEpoch;
use sophia_wake::{Notifier, Wake};
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::time::Instant;

mod executor;
mod graveyard;
mod inbox;
mod native;
use executor::Active;
pub(super) use executor::CaptureExecutor;
pub(super) use graveyard::CaptureGraveyard;
use graveyard::Permit;

const MAX_JOBS: usize = 4;
static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct JobId(u64);

#[derive(Debug)]
pub(super) struct Request<P> {
    pub payload: P,
    pub deadline: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SubmitFailure {
    Capacity,
    Closed,
    Generation,
    IdExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StopReason {
    Cancelled,
    Deadline,
    Generation,
    ExecutorClosed,
    RendererRefused,
}

#[derive(Debug)]
pub(super) enum Outcome<P, O> {
    Completed {
        payload: P,
        output: O,
    },
    RolledBack {
        payload: P,
        reason: StopReason,
    },
    CompletedDiscarded {
        payload: P,
        output: O,
        reason: StopReason,
    },
    // Custody is in the renderer graveyard, never in this disposable receipt.
    FailedUncertain,
}

pub(super) struct Completion<P, O> {
    pub id: JobId,
    pub outcome: Outcome<P, O>,
    epoch: Arc<LiveRendererSnapshotEpoch>,
}

struct Receipt<P, O> {
    completion: Completion<P, O>,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    // Successful/rollback jobs occupy a slot until their receipt is taken.
    permit: Option<Permit<P>>,
}

struct Job<P> {
    id: JobId,
    epoch: Arc<LiveRendererSnapshotEpoch>,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    payload: P,
    permit: Permit<P>,
}

impl<P> Job<P> {
    fn forbidden(&self, now: Instant) -> Option<StopReason> {
        forbidden(&self.epoch, &self.cancel, self.deadline, now)
    }
}

fn forbidden(
    epoch: &LiveRendererSnapshotEpoch,
    cancel: &AtomicBool,
    deadline: Instant,
    now: Instant,
) -> Option<StopReason> {
    if !epoch.is_valid() {
        Some(StopReason::Generation)
    } else if cancel.load(Ordering::Acquire) {
        Some(StopReason::Cancelled)
    } else if now >= deadline {
        Some(StopReason::Deadline)
    } else {
        None
    }
}

// No public constructor, callback, or supplied-completion entry point.
struct ObservedCompletion {
    id: JobId,
    epoch: Arc<LiveRendererSnapshotEpoch>,
}

struct Pending {
    id: JobId,
    cancel: Arc<AtomicBool>,
}

pub(super) struct CaptureInbox<P, O> {
    sender: mpsc::SyncSender<Job<P>>,
    completed: mpsc::Receiver<Receipt<P, O>>,
    pending: [Option<Pending>; MAX_JOBS],
    epoch: Arc<LiveRendererSnapshotEpoch>,
    graveyard: Arc<CaptureGraveyard<P>>,
    request_wake: Notifier,
    completion_wake: Wake,
    alive: Arc<AtomicBool>,
    settled: Arc<AtomicBool>,
    submission: Arc<Mutex<()>>,
    closed: bool,
}

/// Construct on the renderer owner. Only the inbox may cross owners. The
/// graveyard must be shared across replacement mailboxes and retained by the
/// renderer until source reads are proven ended, including on unwind.
/// EGL context/display teardown alone does not establish that proof.
pub(super) fn channel<P, O>(
    epoch: Arc<LiveRendererSnapshotEpoch>,
    graveyard: Arc<CaptureGraveyard<P>>,
) -> io::Result<(CaptureInbox<P, O>, CaptureExecutor<P, O>)> {
    let request_wake = Wake::new()?;
    let completion_wake = Wake::new()?;
    let (sender, requests) = mpsc::sync_channel(MAX_JOBS);
    let (completed, receiver) = mpsc::sync_channel(MAX_JOBS);
    let alive = Arc::new(AtomicBool::new(true));
    let settled = Arc::new(AtomicBool::new(false));
    let submission = Arc::new(Mutex::new(()));
    let inbox = CaptureInbox {
        sender,
        completed: receiver,
        pending: std::array::from_fn(|_| None),
        epoch: epoch.clone(),
        graveyard: graveyard.clone(),
        request_wake: request_wake.notifier(),
        completion_wake,
        alive: alive.clone(),
        settled: settled.clone(),
        submission: submission.clone(),
        closed: false,
    };
    let executor = CaptureExecutor {
        requests,
        completed,
        active: Vec::<Active<P>>::with_capacity(MAX_JOBS),
        epoch,
        graveyard,
        request_wake,
        completion_wake: inbox.completion_wake.notifier(),
        alive,
        settled,
        submission,
        owner_thread: std::marker::PhantomData,
    };
    Ok((inbox, executor))
}

impl<P, O> AsFd for CaptureInbox<P, O> {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.completion_wake.as_fd()
    }
}

impl<P, O> Drop for CaptureInbox<P, O> {
    fn drop(&mut self) {
        for pending in self.pending.iter().flatten() {
            pending.cancel.store(true, Ordering::Release);
        }
        // The executor must still reconcile already-submitted jobs.
        self.request_wake.notify();
    }
}

#[cfg(test)]
#[path = "../tests/support/shell_capture_handoff.rs"]
mod tests;
