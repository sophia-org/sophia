//! G3-5d d-2a, test-support: instrumentation of the real validator for
//! end-to-end combined controls (design REV2 R1 and R7, REV3 section 2). A
//! pre-validation hold armed for the next combined admission, a loop-head
//! progress counter and a per-worker serial. None of it changes what the
//! validator decides or sends.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

pub(crate) const CHECKPOINT_WAIT: Duration = Duration::from_secs(30);

/// The validator's half of a pre-validation hold.
pub struct Checkpoint {
    pub(crate) entered: mpsc::SyncSender<()>,
    pub(crate) resume: mpsc::Receiver<()>,
}

/// The control's half. Dropping it while the validator is held ends the
/// validator thread (see `park`), which WAKE-SEAM.md records as its exit.
pub struct CheckpointHandle {
    entered: mpsc::Receiver<()>,
    resume: Option<mpsc::SyncSender<()>>,
}

/// A hold and its control handle.
pub fn checkpoint() -> (Checkpoint, CheckpointHandle) {
    let (entered, arrival) = mpsc::sync_channel(1);
    let (resume, release) = mpsc::sync_channel(1);
    (
        Checkpoint {
            entered,
            resume: release,
        },
        CheckpointHandle {
            entered: arrival,
            resume: Some(resume),
        },
    )
}

impl CheckpointHandle {
    /// Whether the validator reached the hold within `wait`.
    pub fn entered(&self, wait: Duration) -> bool {
        self.entered.recv_timeout(wait).is_ok()
    }

    /// Lets the held validator go on to validate, once.
    pub fn release(&mut self) {
        if let Some(resume) = self.resume.take() {
            let _ = resume.try_send(());
        }
    }
}

/// Holds the real validator before validation until the control releases it.
pub(super) fn park<P>(route: Option<&mut super::Route<P>>) {
    if let Some(checkpoint) = route.and_then(|route| route.checkpoint.take()) {
        checkpoint
            .entered
            .send(())
            .expect("checkpoint owner present");
        checkpoint
            .resume
            .recv_timeout(CHECKPOINT_WAIT)
            .expect("fixture checkpoint deliberately disconnected or timed out");
    }
}

/// How many times the validator thread has reached the top of its loop.
/// It counts before `recv`, so a job's whole iteration, including any
/// completion send and notification, precedes the next count: n after n - 1
/// complete iterations, 1 at an idle start.
#[derive(Clone, Default)]
pub struct ValidatorProgress(Arc<(Mutex<u64>, Condvar)>);

impl ValidatorProgress {
    pub(crate) fn enter(&self) {
        let (count, changed) = &*self.0;
        *count.lock().unwrap_or_else(|poison| poison.into_inner()) += 1;
        changed.notify_all();
    }

    pub fn value(&self) -> u64 {
        *self.0.0.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    /// Whether the count reached `at_least` within `wait`.
    pub fn wait_for(&self, at_least: u64, wait: Duration) -> bool {
        let (count, changed) = &*self.0;
        let end = Instant::now() + wait;
        let mut current = count.lock().unwrap_or_else(|poison| poison.into_inner());
        while *current < at_least {
            let Some(left) = end.checked_duration_since(Instant::now()) else {
                return false;
            };
            current = changed
                .wait_timeout(current, left)
                .unwrap_or_else(|poison| poison.into_inner())
                .0;
        }
        true
    }
}

static NEXT_VALIDATOR: AtomicU64 = AtomicU64::new(1);

/// What a validator holds for its controls.
pub(crate) struct ValidatorHooks {
    /// Distinct per worker in this process; job numbers are per worker, so a
    /// trace names a job by (serial, job).
    pub(crate) serial: u64,
    pub(crate) progress: ValidatorProgress,
    /// Taken by the next combined admission that carries no hold of its own.
    pub(crate) armed: Option<Checkpoint>,
}

impl ValidatorHooks {
    pub(crate) fn new() -> Self {
        Self {
            serial: NEXT_VALIDATOR.fetch_add(1, Ordering::Relaxed),
            progress: ValidatorProgress::default(),
            armed: None,
        }
    }
}
