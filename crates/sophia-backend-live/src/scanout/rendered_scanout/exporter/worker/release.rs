struct DiscardedWorkerLease {
    output: LiveRendererWorkerOutputKey,
    lease_id: LiveRendererWorkerLeaseId,
    slot_token: LiveRendererFrameSlotToken,
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
}
