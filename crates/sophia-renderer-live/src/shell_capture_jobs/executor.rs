use super::*;
use std::marker::PhantomData;
use std::rc::Rc;

pub(super) struct Active<P> {
    job: Job<P>,
    submitted: bool,
    forbidden: Option<StopReason>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum NextJob {
    Ready(JobId),
    Settled,
    Empty,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum ProofFailure {
    UnknownJob,
    Generation,
    NotSubmitted,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum BeginFailure {
    UnknownJob,
    AlreadySubmitted,
    RolledBack(StopReason),
}

pub(crate) struct CaptureExecutor<P, O> {
    pub(super) requests: mpsc::Receiver<Job<P>>,
    pub(super) completed: mpsc::SyncSender<Receipt<P, O>>,
    pub(super) active: Vec<Active<P>>,
    pub(super) epoch: Arc<LiveRendererSnapshotEpoch>,
    pub(super) graveyard: Arc<CaptureGraveyard<P>>,
    pub(super) request_wake: Wake,
    pub(super) completion_wake: Notifier,
    pub(super) alive: Arc<AtomicBool>,
    pub(super) settled: Arc<AtomicBool>,
    pub(super) submission: Arc<Mutex<()>>,
    // The renderer/context owner drives this; never a second worker thread.
    pub(super) owner_thread: PhantomData<Rc<()>>,
}

impl<P, O> CaptureExecutor<P, O> {
    pub(super) fn active_payload_mut(&mut self, id: JobId) -> Option<&mut P> {
        self.active
            .iter_mut()
            .find(|a| a.job.id == id)
            .map(|a| &mut a.job.payload)
    }

    /// At most one request per call. The driver supplies its per-turn budget.
    pub(super) fn next(&mut self, now: Instant) -> io::Result<NextJob> {
        self.request_wake.clear()?;
        let Ok(job) = self.requests.try_recv() else {
            return Ok(NextJob::Empty);
        };
        // If the driver reaches its turn budget, remaining work must wake it.
        self.request_wake.notifier().notify();
        if let Some(reason) = job.forbidden(now) {
            self.rollback(job, reason);
            return Ok(NextJob::Settled);
        }
        let id = job.id;
        self.active.push(Active {
            job,
            submitted: false,
            forbidden: None,
        });
        Ok(NextJob::Ready(id))
    }

    /// Must precede import and every other operation that could reach the GPU.
    /// Once this returns, even unwinding requires quarantine or exact completion.
    pub(super) fn begin(&mut self, id: JobId, now: Instant) -> Result<&mut P, BeginFailure> {
        let index = self
            .active
            .iter()
            .position(|a| a.job.id == id)
            .ok_or(BeginFailure::UnknownJob)?;
        if self.active[index].submitted {
            return Err(BeginFailure::AlreadySubmitted);
        }
        if let Some(reason) = self.active[index]
            .forbidden
            .or_else(|| self.active[index].job.forbidden(now))
        {
            let active = self.active.remove(index);
            self.rollback(active.job, reason);
            return Err(BeginFailure::RolledBack(reason));
        }
        self.active[index].submitted = true;
        Ok(&mut self.active[index].job.payload)
    }

    /// Deadlines and cancellation forbid publication; they do not prove reads ended.
    pub(super) fn observe_cancellations(&mut self, now: Instant) {
        for active in &mut self.active {
            active.forbidden = active.forbidden.or_else(|| active.job.forbidden(now));
        }
    }

    /// Only the private adapter uses this, after native begin returned the
    /// original request without attempting any source-using operation.
    pub(super) fn refused_before_use(&mut self, id: JobId) -> Result<(), ProofFailure> {
        let index = self
            .active
            .iter()
            .position(|a| a.job.id == id)
            .ok_or(ProofFailure::UnknownJob)?;
        let active = self.active.remove(index);
        self.rollback(active.job, StopReason::RendererRefused);
        Ok(())
    }

    pub(super) fn complete(
        &mut self,
        proof: ObservedCompletion,
        output: O,
        now: Instant,
    ) -> Result<(), (ProofFailure, O)> {
        let Some(index) = self.active.iter().position(|a| a.job.id == proof.id) else {
            return Err((ProofFailure::UnknownJob, output));
        };
        let active = &self.active[index];
        if !Arc::ptr_eq(&proof.epoch, &active.job.epoch) {
            return Err((ProofFailure::Generation, output));
        }
        if !active.submitted {
            return Err((ProofFailure::NotSubmitted, output));
        }
        let active = self.active.remove(index);
        let reason = active.forbidden.or_else(|| active.job.forbidden(now));
        let Job {
            id,
            epoch,
            cancel,
            deadline,
            payload,
            permit,
        } = active.job;
        let outcome = match reason {
            Some(reason) => Outcome::CompletedDiscarded {
                payload,
                output,
                reason,
            },
            None => Outcome::Completed { payload, output },
        };
        self.emit(Receipt {
            completion: Completion { id, epoch, outcome },
            cancel,
            deadline,
            permit: Some(permit),
        });
        Ok(())
    }

    pub(super) fn uncertain(&mut self, id: JobId) -> Result<(), ProofFailure> {
        let Some(index) = self.active.iter().position(|a| a.job.id == id) else {
            return Err(ProofFailure::UnknownJob);
        };
        let active = self.active.remove(index);
        if !active.submitted {
            self.rollback(active.job, StopReason::ExecutorClosed);
        } else {
            self.quarantine(active.job);
        }
        Ok(())
    }

    fn rollback(&self, job: Job<P>, reason: StopReason) {
        let Job {
            id,
            epoch,
            cancel,
            deadline,
            payload,
            permit,
        } = job;
        self.emit(Receipt {
            completion: Completion {
                id,
                epoch,
                outcome: Outcome::RolledBack { payload, reason },
            },
            cancel,
            deadline,
            permit: Some(permit),
        });
    }

    fn quarantine(&self, job: Job<P>) {
        let Job {
            id,
            epoch,
            cancel,
            deadline,
            payload,
            permit,
        } = job;
        self.epoch.invalidate();
        self.graveyard.quarantine(id, payload, permit);
        self.emit(Receipt {
            completion: Completion {
                id,
                epoch,
                outcome: Outcome::FailedUncertain,
            },
            cancel,
            deadline,
            permit: None,
        });
    }

    fn emit(&self, receipt: Receipt<P, O>) {
        // One terminal per job, and all untaken terminals retain a slot: the
        // bounded result queue cannot be full in a valid execution.
        // A disconnected owner may drop safe rollback/completion payloads:
        // their GPU use never began or has proven completion. Uncertain
        // payloads are already in the graveyard, outside the receipt.
        if let Err(mpsc::TrySendError::Full(_)) = self.completed.try_send(receipt) {
            self.alive.store(false, Ordering::Release);
            self.epoch.invalidate();
        }
        // Publish before notifying, including when the owner disconnected.
        self.completion_wake.notify();
    }

    /// Explicit shutdown and Drop share the same bounded settlement path.
    pub(super) fn close(&mut self) {
        {
            let _gate = self.submission.lock().unwrap_or_else(|e| e.into_inner());
            self.alive.store(false, Ordering::Release);
        }
        self.epoch.invalidate();
        while let Some(active) = self.active.pop() {
            if active.submitted {
                self.quarantine(active.job);
            } else {
                self.rollback(active.job, StopReason::ExecutorClosed);
            }
        }
        while let Ok(job) = self.requests.try_recv() {
            self.rollback(job, StopReason::ExecutorClosed);
        }
        self.settled.store(true, Ordering::Release);
        self.completion_wake.notify();
    }
}

impl<P, O> AsFd for CaptureExecutor<P, O> {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.request_wake.as_fd()
    }
}

impl<P, O> Drop for CaptureExecutor<P, O> {
    fn drop(&mut self) {
        self.close();
    }
}
