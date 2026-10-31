//! One bounded validation worker shared by an owner, never one thread per image.
use super::{IsbPixelValidation, ValidatedIsbPixels};
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
pub enum IsbValidationFailure {
    Cancelled,
    Timeout,
    Malformed,
    SizeMismatch,
    GenerationChanged,
}

pub struct IsbValidationCompletion {
    pub job: u64,
    pub result: Result<ValidatedIsbPixels, IsbValidationFailure>,
}

enum Source {
    Isb(OwnedFd),
    Ring(super::OwnedRingRead),
    #[cfg(feature = "experimental-draw")]
    Draw(sophia_draw::Batch),
}

impl Source {
    fn into_isb(self) -> OwnedFd {
        match self {
            Self::Isb(fd) => fd,
            Self::Ring(_) => unreachable!("ISB submission returns its own source"),
            #[cfg(feature = "experimental-draw")]
            Self::Draw(_) => unreachable!("ISB submission returns its own source"),
        }
    }
}

struct Job {
    id: u64,
    source: Source,
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
pub enum IsbSubmitFailure {
    Capacity,
    Invalid,
    Closed,
}

/// A successful submit owns the source until a completion is taken. The owner
/// must reconcile every completion with its resource reservation, even after
/// cancellation. Keep this pool alive while retired epochs still own jobs.
pub struct IsbValidationWorker {
    sender: mpsc::SyncSender<Job>,
    completed: mpsc::Receiver<IsbValidationCompletion>,
    pending: [Option<Pending>; 4],
    next_id: u64,
    max_jobs: usize,
    max_bytes: usize,
    wake: Wake,
}

impl IsbValidationWorker {
    pub fn new(max_jobs: usize, max_bytes: usize) -> io::Result<Self> {
        Self::with_validation_budget(max_jobs, max_bytes, super::ISB_PIXEL_VALIDATION_BYTES)
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
                "ISB worker bounds",
            ));
        }
        let wake = Wake::new()?;
        let notify = wake.notifier();
        let (sender, jobs) = mpsc::sync_channel::<Job>(max_jobs);
        let (results, completed) = mpsc::sync_channel(max_jobs);
        // No callback or owner state enters this thread. Each job returns one
        // owned result before the worker starts another; the queue is bounded.
        std::thread::Builder::new()
            .name("isb-validation".into())
            .spawn(move || {
                let _exit = NotifyOnExit(notify.clone());
                let _cost_exit = crate::shell_work_cost::WorkerExit;
                // Declared after the guard so the sender drops first on unwind.
                let completion_sender = results;
                #[cfg(feature = "experimental-draw")]
                let mut raster = None;
                while let Ok(job) = jobs.recv() {
                    let id = job.id;
                    let cost_bytes = job.bytes as u64;
                    let result = validate(
                        job,
                        validation_bytes,
                        #[cfg(feature = "experimental-draw")]
                        &mut raster,
                    );
                    crate::shell_work_cost::job_completed(cost_bytes, result.is_ok());
                    if completion_sender
                        .send(IsbValidationCompletion { job: id, result })
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
    ) -> Result<u64, (IsbSubmitFailure, OwnedFd)> {
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
    pub fn reserve(&mut self, bytes: usize, deadline: Instant) -> Result<u64, IsbSubmitFailure> {
        if bytes == 0
            || !bytes.is_multiple_of(4)
            || bytes > self.max_bytes
            || self.next_id == u64::MAX
        {
            return Err(IsbSubmitFailure::Invalid);
        }
        let Some(slot) = self.pending[..self.max_jobs]
            .iter_mut()
            .find(|slot| slot.is_none())
        else {
            return Err(IsbSubmitFailure::Capacity);
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
    ) -> Result<(), (IsbSubmitFailure, OwnedFd)> {
        let Some(pending) = self
            .pending
            .iter_mut()
            .flatten()
            .find(|pending| pending.id == id && !pending.submitted)
        else {
            return Err((IsbSubmitFailure::Invalid, source));
        };
        let job = Job {
            id,
            source: Source::Isb(source),
            bytes: pending.bytes,
            deadline: pending.deadline,
            cancelled: pending.cancelled.clone(),
        };
        match self.sender.try_send(job) {
            Ok(()) => {
                pending.submitted = true;
                Ok(())
            }
            Err(mpsc::TrySendError::Full(job)) => {
                Err((IsbSubmitFailure::Capacity, job.source.into_isb()))
            }
            Err(mpsc::TrySendError::Disconnected(job)) => {
                Err((IsbSubmitFailure::Closed, job.source.into_isb()))
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
    pub fn take_completed(&mut self) -> io::Result<Option<IsbValidationCompletion>> {
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

impl Drop for IsbValidationWorker {
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
    #[cfg(feature = "experimental-draw")] raster: &mut Option<sophia_draw::Rasterizer>,
) -> Result<ValidatedIsbPixels, IsbValidationFailure> {
    crate::shell_work_cost::result(
        crate::shell_work_cost::Stage::Validate,
        job.bytes as u64,
        || {
            let check = || {
                if job.cancelled.load(Ordering::Acquire) {
                    Err(IsbValidationFailure::Cancelled)
                } else if Instant::now() >= job.deadline {
                    Err(IsbValidationFailure::Timeout)
                } else {
                    Ok(())
                }
            };
            check()?;
            let mut validation = match job.source {
                Source::Isb(fd) => sealed_validation(fd, job.bytes)?,
                Source::Ring(read) => IsbPixelValidation::from_owned(read.copy(check)?)
                    .map_err(|_| IsbValidationFailure::Malformed)?,
                #[cfg(feature = "experimental-draw")]
                Source::Draw(batch) => sealed_validation(
                    draw_source(
                        raster.get_or_insert_with(sophia_draw::Rasterizer::default),
                        &batch,
                        || check().is_err(),
                    )?,
                    job.bytes,
                )?,
            };
            crate::shell_work_cost::result(
                crate::shell_work_cost::Stage::Pixels,
                job.bytes as u64,
                || {
                    loop {
                        check()?;
                        let complete = validation
                            .step_with_budget(validation_bytes)
                            .map_err(|_| IsbValidationFailure::Malformed)?;
                        if complete {
                            check()?;
                            return validation
                                .finish()
                                .map_err(|_| IsbValidationFailure::Malformed);
                        }
                    }
                },
            )
        },
    )
}

fn sealed_validation(
    source: OwnedFd,
    bytes: usize,
) -> Result<IsbPixelValidation, IsbValidationFailure> {
    let mapping = SealedMapping::map(source, bytes).map_err(|error| {
        if error.raw_os_error() == Some(rustix::io::Errno::MSGSIZE.raw_os_error()) {
            IsbValidationFailure::SizeMismatch
        } else {
            IsbValidationFailure::Malformed
        }
    })?;
    IsbPixelValidation::new(mapping).map_err(|_| IsbValidationFailure::Malformed)
}
mod owned_copy;

#[cfg(feature = "experimental-draw")]
fn draw_source(
    raster: &mut sophia_draw::Rasterizer,
    batch: &sophia_draw::Batch,
    cancelled: impl Fn() -> bool,
) -> Result<OwnedFd, IsbValidationFailure> {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    use std::io::Write;
    let bytes = raster
        .render(batch, cancelled)
        .map_err(|error| match error {
            sophia_draw::Error::Cancelled => IsbValidationFailure::Cancelled,
            _ => IsbValidationFailure::Malformed,
        })?;
    let mut file = std::fs::File::from(
        memfd_create(
            "sophia-draw",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .map_err(|_| IsbValidationFailure::Malformed)?,
    );
    file.write_all(&bytes)
        .map_err(|_| IsbValidationFailure::Malformed)?;
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .map_err(|_| IsbValidationFailure::Malformed)?;
    Ok(file.into())
}

#[cfg(feature = "experimental-draw")]
impl IsbValidationWorker {
    /// Raster work uses the same bounded slots, cancellation and wake source.
    /// The owner has already charged the exact output size to its resource store.
    pub(crate) fn submit_draw_reserved(
        &mut self,
        id: u64,
        batch: sophia_draw::Batch,
    ) -> Result<(), IsbSubmitFailure> {
        batch.validate().map_err(|_| IsbSubmitFailure::Invalid)?;
        let pending = self
            .pending
            .iter_mut()
            .flatten()
            .find(|p| p.id == id && !p.submitted && p.bytes == batch.pixel_bytes())
            .ok_or(IsbSubmitFailure::Invalid)?;
        let job = Job {
            id,
            source: Source::Draw(batch),
            bytes: pending.bytes,
            deadline: pending.deadline,
            cancelled: pending.cancelled.clone(),
        };
        match self.sender.try_send(job) {
            Ok(()) => {
                pending.submitted = true;
                Ok(())
            }
            Err(mpsc::TrySendError::Full(_)) => Err(IsbSubmitFailure::Capacity),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(IsbSubmitFailure::Closed),
        }
    }
}
