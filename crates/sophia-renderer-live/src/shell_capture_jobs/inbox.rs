use super::*;

pub(super) struct CompletionBatch<P, O> {
    pub completions: Vec<Completion<P, O>>,
    pub fault: bool,
}

impl<P, O> CaptureInbox<P, O> {
    pub(super) fn submit(
        &mut self,
        request: Request<P>,
    ) -> Result<JobId, (SubmitFailure, Request<P>)> {
        let _gate = self.submission.lock().unwrap_or_else(|e| e.into_inner());
        if self.closed || !self.alive.load(Ordering::Acquire) {
            return Err((SubmitFailure::Closed, request));
        }
        if !self.epoch.is_valid() {
            return Err((SubmitFailure::Generation, request));
        }
        let Some(slot) = self.pending.iter_mut().find(|slot| slot.is_none()) else {
            return Err((SubmitFailure::Capacity, request));
        };
        let Some(permit) = self.graveyard.reserve() else {
            return Err((SubmitFailure::Capacity, request));
        };
        let Ok(id) =
            NEXT_JOB.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        else {
            return Err((SubmitFailure::IdExhausted, request));
        };
        let id = JobId(id);
        let cancel = Arc::new(AtomicBool::new(false));
        let job = Job {
            id,
            epoch: self.epoch.clone(),
            cancel: cancel.clone(),
            deadline: request.deadline,
            payload: request.payload,
            permit,
        };
        match self.sender.try_send(job) {
            Ok(()) => {
                *slot = Some(Pending { id, cancel });
                self.request_wake.notify();
                Ok(id)
            }
            Err(error) => {
                let (reason, job) = match error {
                    mpsc::TrySendError::Full(job) => (SubmitFailure::Capacity, job),
                    mpsc::TrySendError::Disconnected(job) => (SubmitFailure::Closed, job),
                };
                Err((
                    reason,
                    Request {
                        payload: job.payload,
                        deadline: job.deadline,
                    },
                ))
            }
        }
    }

    pub(super) fn cancel(&self, id: JobId) -> bool {
        let Some(pending) = self.pending.iter().flatten().find(|entry| entry.id == id) else {
            return false;
        };
        pending.cancel.store(true, Ordering::Release);
        self.request_wake.notify();
        true
    }

    /// Clear before inspecting the queue and drain the entire bounded batch.
    /// Every returned completion still needs the runtime owner's reconciliation.
    pub(super) fn take_completed(&mut self, now: Instant) -> io::Result<CompletionBatch<P, O>> {
        self.completion_wake.clear()?;
        // A true observation precedes the drain and guarantees all close
        // receipts were already published. Loading after an empty receive
        // could race the final publication and falsely report a missing job.
        let settlement_finished = self.settled.load(Ordering::Acquire);
        let mut completions = Vec::with_capacity(MAX_JOBS);
        let mut fault = false;
        while let Ok(mut receipt) = self.completed.try_recv() {
            let id = receipt.completion.id;
            let slot = self
                .pending
                .iter()
                .position(|slot| slot.as_ref().is_some_and(|p| p.id == id));
            if !Arc::ptr_eq(&receipt.completion.epoch, &self.epoch) || slot.is_none() {
                // An internal producer violated its contract. Do not settle a
                // different slot or release unknown custody. Close admission.
                self.closed = true;
                self.epoch.invalidate();
                fault = true;
                if let Some(permit) = receipt.permit.take() {
                    let payload = match receipt.completion.outcome {
                        Outcome::Completed { payload, .. }
                        | Outcome::CompletedDiscarded { payload, .. }
                        | Outcome::RolledBack { payload, .. } => Some(payload),
                        Outcome::FailedUncertain => None,
                    };
                    if let Some(payload) = payload {
                        self.graveyard.quarantine(id, payload, permit);
                    }
                }
                continue;
            }
            self.pending[slot.unwrap()] = None;
            // Cancellation, expiry, or epoch withdrawal can arrive AFTER the
            // renderer completed and BEFORE the owner takes its receipt.
            if let Some(reason) = forbidden(&self.epoch, &receipt.cancel, receipt.deadline, now)
                && let Outcome::Completed { payload, output } = receipt.completion.outcome
            {
                receipt.completion.outcome = Outcome::CompletedDiscarded {
                    payload,
                    output,
                    reason,
                };
            }
            completions.push(receipt.completion);
            // The mailbox slot ends here; payload storage is still owned by
            // the returned completion, including its snapshot reservation.
        }
        if settlement_finished && self.pending.iter().any(Option::is_some) {
            self.closed = true;
            fault = true;
        }
        Ok(CompletionBatch { completions, fault })
    }
}
