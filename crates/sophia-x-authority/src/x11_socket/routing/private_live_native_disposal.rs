/// Progress through completed native records while the original executor is
/// still alive. The source dependency scan examines one pair per visit.
#[cfg(unix)]
#[derive(Default)]
struct PrivateLiveNativeDisposal {
    index: usize,
    scan: PrivateTerminalDriveCursor,
    scanning: bool,
    due: bool,
}

#[cfg(unix)]
impl PrivateLiveNativeDisposal {
    fn invalidate(&mut self) {
        self.scanning = false;
    }

    fn next(&mut self) {
        self.index = self.index.saturating_add(1);
        self.invalidate();
    }
}

#[cfg(unix)]
impl PrivateDeliveryCustody {
    fn writer_settled(&self) -> bool {
        self.attempt.is_none()
            && self.pending.is_none()
            && self
                .completion
                .as_ref()
                .and_then(|cell| cell.answer())
                .is_some_and(|answer| {
                    self.recipient_termination || matches!(
                        answer.outcome,
                        XAuthorityInputDeliveryOutcome::Flushed
                            | XAuthorityInputDeliveryOutcome::ClientDisconnected
                    )
                })
    }
}

#[cfg(unix)]
impl PrivateTerminalInventory {
    /// Only cached facts are read before charging. A held or unanswered row
    /// alone does not turn an otherwise idle service into cleanup traffic.
    fn owes_live_native_disposal(&self) -> bool {
        self.settling.iter().any(|release| {
            release.native_recorded
                && release.custody.attempt.is_none()
                && (release.custody.recipient_termination || matches!(
                    release.custody.outcome_seen,
                    Some(
                        XAuthorityInputDeliveryOutcome::Flushed
                            | XAuthorityInputDeliveryOutcome::ClientDisconnected
                    )
                ))
        })
    }

    fn native_shape(&self) -> (usize, usize, bool) {
        (
            self.holds.len(),
            self.settling.len(),
            self.native_pending.is_some(),
        )
    }

    /// Invoked only after the live service's existing charge/watch hook.
    /// Actual native and writer facts precede the ledger absence check. This
    /// path never derives recipient proof from a stopped owner's termination.
    fn dispose_live_native_one(&mut self) -> bool {
        let mut progress = std::mem::take(&mut self.live_disposal);
        let disposed = self.visit_live_native_disposal(&mut progress);
        self.live_disposal = progress;
        disposed
    }

    fn visit_live_native_disposal(&mut self, progress: &mut PrivateLiveNativeDisposal) -> bool {
        if self.settling.is_empty() {
            progress.invalidate();
            return false;
        }
        progress.index %= self.settling.len();
        let release = &self.settling[progress.index];
        let native_proved = release
            .native
            .as_ref()
            .and_then(PrivateNativeHold::proof)
            .is_some_and(|proof| proof.incarnation() == release.incarnation);
        if !native_proved
            || !release.native_recorded
            || !release.custody.writer_settled()
            || !release
                .press_custody
                .as_ref()
                .is_some_and(PrivateDeliveryCustody::writer_settled)
            || self.attempt_custody.is_some()
        {
            progress.next();
            return false;
        }
        let candidate = self.holds.len() + progress.index;
        if !progress.scanning {
            progress.scan.next_recipient(candidate);
            progress.scanning = true;
        }
        if !self.native_disposal_ready(&mut progress.scan) {
            if progress.scan.recipient != candidate {
                progress.next();
            }
            return false;
        }
        let incarnation = self.settling[progress.index].incarnation;
        let settled = self.controller.under_common_as_origin(|authority, issuer| {
            authority.reconciliation_record_present(issuer, incarnation)
        });
        if !matches!(settled, Ok(Ok(false))) {
            progress.next();
            return false;
        }
        self.settling.remove(progress.index);
        self.shared_activation.invalidate();
        progress.invalidate();
        true
    }
}
