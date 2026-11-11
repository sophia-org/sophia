struct DiscardedWorkerLease {
    output: LiveRendererWorkerOutputKey,
    lease_id: LiveRendererWorkerLeaseId,
    slot_token: LiveRendererFrameSlotToken,
}

/// The combined frame-job Release obligation for this facade: admission
/// reserves it, an unused speculative lease is retained in it, and it frees
/// only after the Release command is enqueued. Enqueue is not slot freedom.
enum CombinedReleaseCell {
    Free,
    Reserved,
    Retained(DiscardedWorkerLease),
}

#[derive(Debug, Eq, PartialEq)]
enum CombinedReleaseFlush {
    NoRelease,
    Enqueued,
    Full,
    Disconnected,
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

    /// One combined job per facade, and only when this facade could render it.
    // No production caller until the stage 4c owner join.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn reserve_combined_release(&mut self) -> bool {
        if self.quarantined
            || self.stalled.is_some()
            || self.in_flight.is_some()
            || self.claim.is_detached()
            || !matches!(self.combined_release, CombinedReleaseCell::Free)
        {
            return false;
        }
        self.combined_release = CombinedReleaseCell::Reserved;
        true
    }

    /// Refused results come back intact; their frame or failure still
    /// belongs to the owner.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn retain_combined_result(&mut self, result: WorkerResult) -> Option<WorkerResult> {
        if result.output != self.output
            || !matches!(self.combined_release, CombinedReleaseCell::Reserved)
        {
            return Some(result);
        }
        let WorkerOutcome::Exported {
            lease_id,
            slot_token,
            ..
        } = &result.outcome
        else {
            return Some(result);
        };
        self.combined_release = CombinedReleaseCell::Retained(DiscardedWorkerLease {
            output: result.output,
            lease_id: *lease_id,
            slot_token: *slot_token,
        });
        None
    }

    /// Full and Disconnected both keep the lease: a closed queue is not proof
    /// that the slot was retired.
    fn flush_combined_release(&mut self) -> CombinedReleaseFlush {
        let CombinedReleaseCell::Retained(release) = &self.combined_release else {
            return CombinedReleaseFlush::NoRelease;
        };
        let sent = self.core.command_sender.try_send(WorkerCommand::Release {
            output: release.output,
            lease_id: release.lease_id,
            slot_token: release.slot_token,
        });
        let flush = match sent {
            Ok(()) => {
                self.combined_release = CombinedReleaseCell::Free;
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
