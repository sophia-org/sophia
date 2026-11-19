struct DiscardedWorkerLease {
    output: LiveRendererWorkerOutputKey,
    lease_id: LiveRendererWorkerLeaseId,
    slot_token: LiveRendererFrameSlotToken,
}

/// One combined admission on this facade: the facade-local ticket and the
/// runtime job. Job numbers repeat across validators; the ticket does not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CombinedAdmission {
    ticket: u64,
    job: u64,
}

/// The combined frame-job Release obligation for this facade. Admission
/// reserves it under a ticket, settlement holds an exported result under
/// that admission, and giving the result up retains its lease for Release.
/// It frees only once the Release is enqueued, which is not slot freedom.
// Held is the frame the owner may still accept; boxing it would add an
// allocation per frame on the path this lane measures.
#[allow(clippy::large_enum_variant)]
enum CombinedReleaseCell {
    Free,
    Reserved(u64),
    Held {
        admission: CombinedAdmission,
        result: WorkerResult,
    },
    Retained {
        admission: CombinedAdmission,
        lease: DiscardedWorkerLease,
    },
}

/// Owner-local evidence beside the cell: the last admission ticket issued,
/// and the admission whose Release either caller of the shared flush
/// enqueued.
#[derive(Default)]
struct CombinedReceipt {
    admissions: u64,
    released: Option<CombinedAdmission>,
}

#[derive(Debug, Eq, PartialEq)]
enum CombinedReleaseFlush {
    NoRelease,
    Enqueued,
    Full,
    Disconnected,
}

/// What maintenance found for one job's Release (AUTHORITY-STEP3.md rev 5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

fn retains(cell: &CombinedReleaseCell, admission: CombinedAdmission) -> bool {
    matches!(cell, CombinedReleaseCell::Retained { admission: held, .. } if *held == admission)
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
            || self.combined_receipt.released.is_some()
            || !matches!(self.combined_release, CombinedReleaseCell::Free)
        {
            return None;
        }
        self.combined_receipt.admissions = self.combined_receipt.admissions.wrapping_add(1);
        self.combined_release = CombinedReleaseCell::Reserved(self.combined_receipt.admissions);
        #[cfg(feature = "experimental-combined-frame-job")]
        {
            self.metrics.combined.admissions = self.metrics.combined.admissions.saturating_add(1);
        }
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
        #[cfg(feature = "experimental-combined-frame-job")]
        {
            self.metrics.combined.cancels = self.metrics.combined.cancels.saturating_add(1);
        }
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
        self.combined_release = CombinedReleaseCell::Held {
            admission: CombinedAdmission { ticket, job },
            result,
        };
        #[cfg(feature = "experimental-combined-frame-job")]
        {
            self.metrics.combined.holds = self.metrics.combined.holds.saturating_add(1);
        }
        None
    }

    /// The owner gave up this admission's held result: its lease is retained
    /// for Release and one flush is tried at once (AUTHORITY-STEP3.md rev 4).
    #[cfg_attr(
        not(any(test, feature = "experimental-combined-frame-job")),
        allow(dead_code)
    )]
    fn give_up_combined_held(&mut self, admission: CombinedAdmission) -> CombinedRelease {
        self.combined_release = match std::mem::replace(
            &mut self.combined_release,
            CombinedReleaseCell::Free,
        ) {
            CombinedReleaseCell::Held {
                admission: held,
                result,
            } if held == admission => match result.outcome {
                WorkerOutcome::Exported {
                    lease_id,
                    slot_token,
                    ..
                } => {
                    #[cfg(feature = "experimental-combined-frame-job")]
                    {
                        self.metrics.combined.give_ups =
                            self.metrics.combined.give_ups.saturating_add(1);
                    }
                    CombinedReleaseCell::Retained {
                        admission,
                        lease: DiscardedWorkerLease {
                            output: result.output,
                            lease_id,
                            slot_token,
                        },
                    }
                }
                // Only exported results are held.
                _ => CombinedReleaseCell::Held {
                    admission: held,
                    result,
                },
            },
            other => other,
        };
        self.combined_release_for(admission)
    }

    /// The owner's maintenance entry: the shared flush for this admission
    /// only, and the receipt either caller left. Never `poll()`, so no ordinary result
    /// is taken.
    #[cfg_attr(
        not(any(test, feature = "experimental-combined-frame-job")),
        allow(dead_code)
    )]
    fn combined_release_for(&mut self, admission: CombinedAdmission) -> CombinedRelease {
        if retains(&self.combined_release, admission)
            && self.flush_combined_release() == CombinedReleaseFlush::Disconnected
        {
            return CombinedRelease::Disconnected;
        }
        if self.combined_receipt.released == Some(admission) {
            self.combined_receipt.released = None;
            return CombinedRelease::Transferred;
        }
        if retains(&self.combined_release, admission) {
            CombinedRelease::Pending
        } else {
            CombinedRelease::Mismatch
        }
    }

    /// Test-support (G3-5d d-2b3c): the revision-5 receipt as (admission
    /// ticket, job), read without consuming it.
    #[cfg(all(
        any(test, feature = "test-support"),
        feature = "experimental-combined-frame-job"
    ))]
    pub(crate) fn combined_receipt_for_control(&self) -> Option<(u64, u64)> {
        self.combined_receipt
            .released
            .map(|admission| (admission.ticket, admission.job))
    }

    /// Full and Disconnected both keep the lease: a closed queue is not proof
    /// that the slot was retired. An enqueue, from `poll()` or maintenance,
    /// leaves the admission's receipt.
    fn flush_combined_release(&mut self) -> CombinedReleaseFlush {
        let CombinedReleaseCell::Retained {
            admission,
            lease: release,
        } = &self.combined_release
        else {
            return CombinedReleaseFlush::NoRelease;
        };
        let admission = *admission;
        let sent = self.core.command_sender.try_send(WorkerCommand::Release {
            output: release.output,
            lease_id: release.lease_id,
            slot_token: release.slot_token,
        });
        let flush = match sent {
            Ok(()) => {
                self.combined_release = CombinedReleaseCell::Free;
                self.combined_receipt.released = Some(admission);
                #[cfg(feature = "experimental-combined-frame-job")]
                {
                    self.metrics.combined.release_enqueues =
                        self.metrics.combined.release_enqueues.saturating_add(1);
                }
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
