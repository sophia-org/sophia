struct DiscardedWorkerLease {
    output: LiveRendererWorkerOutputKey,
    lease_id: LiveRendererWorkerLeaseId,
    slot_token: LiveRendererFrameSlotToken,
}

/// The combined frame-job Release obligation for this facade. Admission
/// reserves it under a facade-local ticket, settlement holds an exported
/// result under the owner row's job, and giving that result up retains its
/// lease for Release. It frees only once the Release is enqueued, which is
/// not slot freedom.
// Held is the frame the owner may still accept; boxing it would add an
// allocation per frame on the path this lane measures.
#[allow(clippy::large_enum_variant)]
enum CombinedReleaseCell {
    Free,
    Reserved(u64),
    Held { job: u64, result: WorkerResult },
    Retained { job: u64, lease: DiscardedWorkerLease },
}

/// Owner-local evidence beside the cell: the last admission ticket issued,
/// and the job whose Release either caller of the shared flush enqueued.
#[derive(Default)]
struct CombinedReceipt {
    admissions: u64,
    released_job: Option<u64>,
}

#[derive(Debug, Eq, PartialEq)]
enum CombinedReleaseFlush {
    NoRelease,
    Enqueued,
    Full,
    Disconnected,
}

/// What maintenance found for one job's Release (AUTHORITY-STEP3.md rev 5).
#[derive(Debug, Eq, PartialEq)]
pub enum CombinedRelease {
    /// Enqueued by either caller: the command queue owns the obligation.
    /// It is not proof that the slot is free.
    Transferred,
    /// Still retained behind a full queue; the owner's walk retries.
    Pending,
    /// The worker cannot take it; the row waits for qualified retirement.
    Disconnected,
    /// The cell does not hold this job; the row fails closed.
    Mismatch,
    /// No live facade issued this job; the row waits for qualified retirement.
    NotFound,
}

fn reserved(cell: &CombinedReleaseCell, ticket: u64) -> bool {
    matches!(cell, CombinedReleaseCell::Reserved(reserved) if *reserved == ticket)
}

fn retains(cell: &CombinedReleaseCell, job: u64) -> bool {
    matches!(cell, CombinedReleaseCell::Retained { job: held, .. } if *held == job)
}

impl NativeGbmRendererWorker {
    fn release_discarded_result(&mut self, result: &WorkerResult) {
        if let WorkerOutcome::Exported {
            lease_id,
            slot_token,
            ..
        } = result.outcome
        {
            self.discarded_release = Some(DiscardedWorkerLease {
                output: result.output,
                lease_id,
                slot_token,
            });
            self.flush_discarded_release();
        }
    }

    fn flush_discarded_release(&mut self) {
        let Some(release) = self.discarded_release.take() else {
            return;
        };
        if let Err(error) = self.core.command_sender.try_send(WorkerCommand::Release {
            output: release.output,
            lease_id: release.lease_id,
            slot_token: release.slot_token,
        }) {
            self.core
                .release_enqueue_failures
                .fetch_add(1, Ordering::Relaxed);
            if matches!(error, TrySendError::Full(_)) {
                self.discarded_release = Some(release);
            }
        }
    }

    /// One combined job per facade, and only when this facade could render
    /// it. No successor while an enqueued Release is still unacknowledged.
    // Reached only through the combined frame-job facade (BRIDGE.md 4c-3).
    #[cfg_attr(
        not(any(test, feature = "experimental-combined-frame-job")),
        allow(dead_code)
    )]
    fn reserve_combined_release(&mut self) -> Option<u64> {
        if self.quarantined
            || self.stalled.is_some()
            || self.in_flight.is_some()
            || self.claim.is_detached()
            || self.combined_receipt.released_job.is_some()
            || !matches!(self.combined_release, CombinedReleaseCell::Free)
        {
            return None;
        }
        self.combined_receipt.admissions = self.combined_receipt.admissions.wrapping_add(1);
        self.combined_release = CombinedReleaseCell::Reserved(self.combined_receipt.admissions);
        Some(self.combined_receipt.admissions)
    }

    /// Frees only this admission's unused reservation.
    #[cfg_attr(
        not(any(test, feature = "experimental-combined-frame-job")),
        allow(dead_code)
    )]
    fn cancel_combined_release(&mut self, ticket: u64) -> bool {
        if !reserved(&self.combined_release, ticket)
        {
            return false;
        }
        self.combined_release = CombinedReleaseCell::Free;
        true
    }

    /// Holds this admission's exported result under the owner row's job.
    /// A refused result comes back intact and changes nothing.
    #[cfg_attr(
        not(any(test, feature = "experimental-combined-frame-job")),
        allow(dead_code)
    )]
    fn hold_combined_result(
        &mut self,
        ticket: u64,
        job: u64,
        result: WorkerResult,
    ) -> Option<WorkerResult> {
        if result.output != self.output
            || !matches!(result.outcome, WorkerOutcome::Exported { .. })
            || !reserved(&self.combined_release, ticket)
        {
            return Some(result);
        }
        self.combined_release = CombinedReleaseCell::Held { job, result };
        None
    }

    /// The owner gave up this job's held result: its lease is retained for
    /// Release and one flush is tried at once (AUTHORITY-STEP3.md rev 4).
    #[cfg_attr(
        not(any(test, feature = "experimental-combined-frame-job")),
        allow(dead_code)
    )]
    pub(crate) fn release_combined_held(&mut self, job: u64) -> CombinedRelease {
        self.combined_release = match std::mem::replace(
            &mut self.combined_release,
            CombinedReleaseCell::Free,
        ) {
            CombinedReleaseCell::Held { job: held, result } if held == job => match result.outcome {
                WorkerOutcome::Exported {
                    lease_id,
                    slot_token,
                    ..
                } => CombinedReleaseCell::Retained {
                    job,
                    lease: DiscardedWorkerLease {
                        output: result.output,
                        lease_id,
                        slot_token,
                    },
                },
                // Only exported results are held.
                _ => CombinedReleaseCell::Held { job: held, result },
            },
            other => other,
        };
        self.flush_combined_release_for(job)
    }

    /// The owner's maintenance entry: the shared flush for this job only, and
    /// the receipt either caller left. Never `poll()`, so no ordinary result
    /// is taken.
    #[cfg_attr(
        not(any(test, feature = "experimental-combined-frame-job")),
        allow(dead_code)
    )]
    pub(crate) fn flush_combined_release_for(&mut self, job: u64) -> CombinedRelease {
        if retains(&self.combined_release, job)
            && self.flush_combined_release() == CombinedReleaseFlush::Disconnected
        {
            return CombinedRelease::Disconnected;
        }
        if self.combined_receipt.released_job == Some(job) {
            self.combined_receipt.released_job = None;
            return CombinedRelease::Transferred;
        }
        if retains(&self.combined_release, job) {
            CombinedRelease::Pending
        } else {
            CombinedRelease::Mismatch
        }
    }

    /// Full and Disconnected both keep the lease: a closed queue is not proof
    /// that the slot was retired. An enqueue, from `poll()` or maintenance,
    /// leaves the job's receipt.
    fn flush_combined_release(&mut self) -> CombinedReleaseFlush {
        let CombinedReleaseCell::Retained { job, lease: release } = &self.combined_release else {
            return CombinedReleaseFlush::NoRelease;
        };
        let job = *job;
        let sent = self.core.command_sender.try_send(WorkerCommand::Release {
            output: release.output,
            lease_id: release.lease_id,
            slot_token: release.slot_token,
        });
        let flush = match sent {
            Ok(()) => {
                self.combined_release = CombinedReleaseCell::Free;
                self.combined_receipt.released_job = Some(job);
                return CombinedReleaseFlush::Enqueued;
            }
            Err(TrySendError::Full(_)) => CombinedReleaseFlush::Full,
            Err(TrySendError::Disconnected(_)) => CombinedReleaseFlush::Disconnected,
        };
        self.metrics.combined_release_enqueue_failures = self
            .metrics
            .combined_release_enqueue_failures
            .saturating_add(1);
        flush
    }
}
