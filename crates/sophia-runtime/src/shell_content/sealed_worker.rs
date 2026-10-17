//! One bounded validation worker shared by an owner, never one thread per image.
use super::{SealedPixelValidation, ValidatedSealedPixels};
use sophia_linux_peer::SealedMapping;
use sophia_wake::Wake;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SealedValidationFailure {
    Cancelled,
    Timeout,
    Malformed,
    SizeMismatch,
}

pub struct SealedValidationCompletion {
    pub job: u64,
    pub result: Result<ValidatedSealedPixels, SealedValidationFailure>,
}

struct Job {
    id: u64,
    source: OwnedFd,
    bytes: usize,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}

struct Pending {
    id: u64,
    bytes: usize,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    submitted: bool,
}

/// Result-channel disconnection alone cannot wake an eventfd waiter. Signal
/// on every thread exit, including unwinding, after the result sender drops.
struct NotifyOnExit(sophia_wake::Notifier);
impl Drop for NotifyOnExit {
    fn drop(&mut self) {
        self.0.notify();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SealedSubmitFailure {
    Capacity,
    Invalid,
    Closed,
}

/// A successful submit owns the source until a completion is taken. The owner
/// must reconcile every completion with its resource reservation, even after
/// cancellation. Keep this pool alive while retired epochs still own jobs.
pub struct SealedValidationWorker {
    sender: mpsc::SyncSender<Job>,
    completed: mpsc::Receiver<SealedValidationCompletion>,
    pending: [Option<Pending>; 4],
    next_id: u64,
    max_jobs: usize,
    max_bytes: usize,
    wake: Wake,
}

impl SealedValidationWorker {
    pub fn new(max_jobs: usize, max_bytes: usize) -> io::Result<Self> {
        Self::with_validation_budget(max_jobs, max_bytes, super::SEALED_PIXEL_VALIDATION_BYTES)
    }

    /// Use the published validation chunk budget. Larger budgets are safely
    /// capped at 64 KiB so cancellation remains bounded independently of it.
    pub fn with_validation_budget(
        max_jobs: usize,
        max_bytes: usize,
        validation_bytes: usize,
    ) -> io::Result<Self> {
        if !(1..=4).contains(&max_jobs)
            || max_bytes == 0
            || max_bytes > isize::MAX as usize
            || validation_bytes == 0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "sealed worker bounds",
            ));
        }
        let wake = Wake::new()?;
        let notify = wake.notifier();
        let (sender, jobs) = mpsc::sync_channel::<Job>(max_jobs);
        let (results, completed) = mpsc::sync_channel(max_jobs);
        // No callback or owner state enters this thread. Each job returns one
        // owned result before the worker starts another; the queue is bounded.
        std::thread::Builder::new()
            .name("sealed-validation".into())
            .spawn(move || {
                let _exit = NotifyOnExit(notify.clone());
                // Declared after the guard so the sender drops first on unwind.
                let completion_sender = results;
                while let Ok(job) = jobs.recv() {
                    let id = job.id;
                    let result = validate(job, validation_bytes);
                    if completion_sender
                        .send(SealedValidationCompletion { job: id, result })
                        .is_err()
                    {
                        break;
                    }
                    notify.notify();
                }
            })?;
        Ok(Self {
            sender,
            completed,
            pending: std::array::from_fn(|_| None),
            next_id: 1,
            max_jobs,
            max_bytes,
            wake,
        })
    }

    /// On refusal the caller receives its original descriptor back.
    pub fn submit(
        &mut self,
        source: OwnedFd,
        bytes: usize,
        deadline: Instant,
    ) -> Result<u64, (SealedSubmitFailure, OwnedFd)> {
        let id = match self.reserve(bytes, deadline) {
            Ok(id) => id,
            Err(error) => return Err((error, source)),
        };
        match self.submit_reserved(id, source) {
            Ok(()) => Ok(id),
            Err(error) => {
                self.release_reserved(id);
                Err(error)
            }
        }
    }

    /// Reserve a validation credit before ImageAdmitted. No FD is needed yet;
    /// the original byte bound and absolute deadline are fixed here. The owner
    /// must either submit the source or release this unsent reservation.
    pub fn reserve(&mut self, bytes: usize, deadline: Instant) -> Result<u64, SealedSubmitFailure> {
        if bytes == 0
            || !bytes.is_multiple_of(4)
            || bytes > self.max_bytes
            || self.next_id == u64::MAX
        {
            return Err(SealedSubmitFailure::Invalid);
        }
        let Some(slot) = self.pending[..self.max_jobs]
            .iter_mut()
            .find(|slot| slot.is_none())
        else {
            return Err(SealedSubmitFailure::Capacity);
        };
        let id = self.next_id;
        self.next_id += 1;
        *slot = Some(Pending {
            id,
            bytes,
            deadline,
            cancelled: Arc::new(AtomicBool::new(false)),
            submitted: false,
        });
        Ok(id)
    }

    /// Transfers the Source once, against its already held credit. Refusal
    /// returns the descriptor and leaves the reservation for the owner to
    /// reconcile. Arrival and retries cannot extend its original deadline.
    pub fn submit_reserved(
        &mut self,
        id: u64,
        source: OwnedFd,
    ) -> Result<(), (SealedSubmitFailure, OwnedFd)> {
        let Some(pending) = self
            .pending
            .iter_mut()
            .flatten()
            .find(|pending| pending.id == id && !pending.submitted)
        else {
            return Err((SealedSubmitFailure::Invalid, source));
        };
        let job = Job {
            id,
            source,
            bytes: pending.bytes,
            deadline: pending.deadline,
            cancelled: pending.cancelled.clone(),
        };
        match self.sender.try_send(job) {
            Ok(()) => {
                pending.submitted = true;
                Ok(())
            }
            Err(mpsc::TrySendError::Full(job)) => Err((SealedSubmitFailure::Capacity, job.source)),
            Err(mpsc::TrySendError::Disconnected(job)) => {
                Err((SealedSubmitFailure::Closed, job.source))
            }
        }
    }

    /// No completion is owed for an unsent source. Once submitted, only taking
    /// its completion can release the credit, including after cancellation.
    pub fn release_reserved(&mut self, id: u64) -> bool {
        let Some(slot) = self.pending.iter_mut().find(|slot| {
            slot.as_ref()
                .is_some_and(|pending| pending.id == id && !pending.submitted)
        }) else {
            return false;
        };
        *slot = None;
        true
    }

    pub fn cancel(&self, job: u64) -> bool {
        if let Some(pending) = self
            .pending
            .iter()
            .flatten()
            .find(|pending| pending.id == job)
        {
            pending.cancelled.store(true, Ordering::Release);
            true
        } else {
            false
        }
    }

    pub fn pending(&self) -> usize {
        self.pending.iter().flatten().count()
    }
    pub fn readiness(&self) -> BorrowedFd<'_> {
        self.wake.as_fd()
    }

    /// Drain until None before polling readiness again. Clearing the wake
    /// precedes queue inspection, so a concurrent completion cannot be lost.
    pub fn take_completed(&mut self) -> io::Result<Option<SealedValidationCompletion>> {
        self.wake.clear()?;
        match self.completed.try_recv() {
            Ok(completion) => {
                let slot = self
                    .pending
                    .iter_mut()
                    .find(|slot| {
                        slot.as_ref().is_some_and(|pending| {
                            pending.id == completion.job && pending.submitted
                        })
                    })
                    .expect("worker returns only an outstanding job");
                *slot = None;
                Ok(Some(completion))
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "validation worker ended",
            )),
        }
    }
}

impl Drop for SealedValidationWorker {
    fn drop(&mut self) {
        // Never join on the owner thread. Shutdown cancels bounded units and
        // drops both queues; the worker closes any abandoned source itself.
        for pending in self.pending.iter().flatten() {
            pending.cancelled.store(true, Ordering::Release);
        }
    }
}

fn validate(
    job: Job,
    validation_bytes: usize,
) -> Result<ValidatedSealedPixels, SealedValidationFailure> {
    let check = || {
        if job.cancelled.load(Ordering::Acquire) {
            Err(SealedValidationFailure::Cancelled)
        } else if Instant::now() >= job.deadline {
            Err(SealedValidationFailure::Timeout)
        } else {
            Ok(())
        }
    };
    check()?;
    let mapping = SealedMapping::map(job.source, job.bytes).map_err(|error| {
        if error.raw_os_error() == Some(rustix::io::Errno::MSGSIZE.raw_os_error()) {
            SealedValidationFailure::SizeMismatch
        } else {
            SealedValidationFailure::Malformed
        }
    })?;
    let mut validation =
        SealedPixelValidation::new(mapping).map_err(|_| SealedValidationFailure::Malformed)?;
    loop {
        check()?;
        let complete = validation
            .step_with_budget(validation_bytes)
            .map_err(|_| SealedValidationFailure::Malformed)?;
        if complete {
            check()?;
            return validation
                .finish()
                .map_err(|_| SealedValidationFailure::Malformed);
        }
    }
}
