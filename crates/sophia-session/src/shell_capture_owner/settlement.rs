//! Completion and source-journal ownership share the original bounded slots.
use super::*;
use sophia_protocol::shell_files::ShellSourceDisposition;
use sophia_runtime::ShellTransportError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Publication {
    Journaled,
    /// The exact connection/epoch no longer has a live journal. This never
    /// substitutes for resource settlement or proof that source reads ended.
    Disconnected,
}

impl Settlement {
    fn settle_resource(
        &mut self,
        registry: &mut ContentEpochRegistry,
        now: u64,
    ) -> Result<ShellSourceDisposition, ContentStoreError> {
        if let Some(disposition) = self.release {
            return Ok(disposition);
        }
        let disposition = match self.receipt.outcome() {
            LiveShellCaptureOutcome::Uncertain => return Err(ContentStoreError::Incomplete),
            LiveShellCaptureOutcome::Returned { reason, .. } => {
                let cancelled = self.withdrawn || *reason == LiveShellCaptureStop::Cancelled;
                self.fail_unpublished(registry, now)?;
                if cancelled {
                    ShellSourceDisposition::CancelledBeforeImport
                } else {
                    ShellSourceDisposition::RefusedBeforeUse
                }
            }
            LiveShellCaptureOutcome::Completed { .. } => {
                match self.accept_completed(registry, now) {
                    Ok(()) => ShellSourceDisposition::CaptureComplete,
                    Err(ContentStoreError::ClockRegression) => {
                        return Err(ContentStoreError::ClockRegression);
                    }
                    Err(accept_error) => {
                        // A completed-but-unpublishable copy has proved that
                        // its exact source reads ended. Settle the original
                        // reservation even if the allocation was replaced.
                        // Wrong owner/ticket and clock errors still preserve it.
                        match self.fail_unpublished(registry, now) {
                            Ok(()) => (),
                            Err(ContentStoreError::Incomplete) => {
                                let failure = if matches!(
                                    accept_error,
                                    ContentStoreError::Stale | ContentStoreError::Revoked
                                ) {
                                    ContentCaptureFailure::Withdrawn
                                } else {
                                    ContentCaptureFailure::Renderer
                                };
                                registry.fail_capture(&mut self.reservation, failure, now)?;
                            }
                            Err(error) => return Err(error),
                        }
                        ShellSourceDisposition::DrainedAfterSubmission
                    }
                }
            }
        };
        self.release = Some(disposition);
        Ok(disposition)
    }
}

impl Owner {
    pub(crate) fn pending(&self) -> usize {
        self.pending.iter().flatten().count()
            + self.settling.iter().flatten().count()
            + self.orphaned.len()
    }

    pub(crate) fn awaiting_completion(&self) -> bool {
        self.pending.iter().any(Option::is_some)
    }

    /// Every owner turn, including disconnected epochs. Publication errors
    /// leave the actual receipt/source in its slot. Only a successful journal
    /// append (or the exact dead journal) releases it after resource settlement.
    pub(crate) fn service(
        &mut self,
        registry: &mut ContentEpochRegistry,
        now: u64,
        instant: Instant,
        mut publish: impl FnMut(
            Binding,
            ShellSourceDisposition,
        ) -> Result<Publication, ShellTransportError>,
    ) -> Vec<(Binding, ShellTransportError)> {
        let mut errors = Vec::new();
        self.observe_resource_failures(registry, now, |binding, error| {
            errors.push((binding, error.into()));
        });
        match self.take_completed(instant) {
            Ok(batch) => {
                for settlement in batch.completed {
                    // Submit charges the slot until both obligations settle.
                    let slot = self
                        .settling
                        .iter_mut()
                        .find(|slot| slot.is_none())
                        .expect("bounded capture completion slot");
                    *slot = Some(settlement);
                }
            }
            Err(error) => {
                self.closed = true;
                for p in self.pending.iter().flatten() {
                    self.inbox.cancel(p.job);
                    errors.push((p.binding, ShellTransportError::Io(error.to_string())));
                }
            }
        }
        for slot in &mut self.settling {
            let Some(settlement) = slot.as_mut() else {
                continue;
            };
            let disposition = match settlement.settle_resource(registry, now) {
                Ok(disposition) => disposition,
                Err(ContentStoreError::Incomplete) => continue,
                Err(error) => {
                    errors.push((settlement.binding, error.into()));
                    continue;
                }
            };
            match publish(settlement.binding, disposition) {
                Ok(Publication::Journaled | Publication::Disconnected) => *slot = None,
                Err(ShellTransportError::ContentQueueSaturated) => (),
                Err(error) => errors.push((settlement.binding, error)),
            }
        }
        errors
    }
}
