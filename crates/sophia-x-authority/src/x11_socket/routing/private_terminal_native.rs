// The source obligations a terminal visit answers.
//
// Split by subject from the ordered stepping beside it: that file is about
// moving one decided entry along its queue, this is about the debts a release
// leaves behind once its entry has gone -- a proof to record, a delivery to
// attempt, an attempt to give back. They change for different reasons, and
// the stepping is the part that has to stay readable end to end.

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Claim one delivery attempt and hand its capsule to the recipient.
    ///
    /// ONE RELEASE, ONE ATTEMPT, and the two ends are joined here because
    /// nothing else holds both: the ledger names a debt by its incarnation
    /// and a receipt arrives naming a delivery.
    ///
    /// THE LEDGER SELECTS. This asks only whether anything could be served at
    /// all, then serves whatever debt the ledger's own cursor chose. Choosing
    /// a record here and rejecting the ledger's answer let one unservable
    /// release hide every other behind it.
    ///
    /// The custody slot is empty before the ledger is asked, so the token has
    /// somewhere reserved to go the moment it exists.
    fn attempt_one_delivery(&mut self) -> Option<bool> {
        // Only a cheap "is there anything this executor could serve" -- NOT a
        // choice of which. Picking a record here and then rejecting whatever
        // the ledger chose meant one unbuildable release at the front hid
        // every buildable one behind it, for ever, because the ledger's cursor
        // never moved.
        if !self
            .terminal
            .settling
            .iter()
            .any(PrivateSettlingRelease::owes_delivery_attempt)
        {
            return None;
        }
        // The one slot this driver may hold has to be free before the ledger
        // is asked. Claiming into storage that does not exist yet is a
        // reservation whose custody is not itself reserved.
        if self.terminal.attempt_custody.is_some() {
            return Some(false);
        }
        let mut cursor = self.terminal.attempt_cursor;
        let claimed = self
            .authority()
            .under_common_as_origin(|authority, issuer| {
                authority.claim_next_attempt(issuer, &mut cursor)
            });
        self.terminal.attempt_cursor = cursor;
        let claim = match claimed {
            Ok(Ok(Some(claim))) => claim,
            Ok(Ok(None)) | Ok(Err(_)) | Err(_) => return Some(false),
        };
        // PERSISTED BEFORE ANYTHING ELSE CAN FAIL, into the slot that was
        // already there. No path below holds the token only in a local, and an
        // unwind leaves it here to be answered rather than leaked.
        self.terminal.attempt_custody = Some(PrivateAttemptCustody {
            token: claim.token,
            phase: PrivateAttemptPhase::Unplaced,
        });

        let Some(index) = self
            .terminal
            .settling
            .iter()
            .position(|release| release.incarnation() == claim.hold)
            .filter(|index| self.terminal.settling[*index].owes_delivery_attempt())
        else {
            // The ledger chose a debt this executor cannot serve. Its answer
            // stands; the token stays outstanding until the give-back is
            // confirmed.
            self.relinquish_outstanding_attempt(claim.token);
            return Some(false);
        };

        // The destination slot is prepared before anything is taken from the
        // hold, so an emission never leaves its obligation with nowhere to be.
        if self.terminal.settling[index].pending.is_none() {
            let taken = self.terminal.settling[index]
                .native_mut()
                .and_then(private_native::Hold::take_release_emission);
            let Some(emission) = taken else {
                self.relinquish_outstanding_attempt(claim.token);
                return Some(false);
            };
            let release = &mut self.terminal.settling[index];
            match XAuthorityOrderedDelivery::from_emission(emission) {
                Ok(capsule) => {
                    release.pending = Some(PrivatePendingDelivery::Capsule(capsule));
                    release.dispatch = PrivateDispatchPhase::Pending;
                }
                Err((cause, emission)) => {
                    // Retained rather than dropped. It cannot be wrapped now,
                    // and it is still the only copy of an event decided at a
                    // moment that has passed.
                    release.pending = Some(PrivatePendingDelivery::Unwrapped { emission, cause });
                    release.dispatch = PrivateDispatchPhase::Unwrappable;
                    self.relinquish_outstanding_attempt(claim.token);
                    return Some(false);
                }
            }
        }
        if !matches!(
            self.terminal.settling[index].pending,
            Some(PrivatePendingDelivery::Capsule(_))
        ) {
            self.relinquish_outstanding_attempt(claim.token);
            return Some(false);
        }

        let recipient = self.terminal.settling[index].reached().client();
        let admission = self.terminal.settling[index]
            .delivery()
            .and_then(|delivery| self.broker.registry.input_recovery.ticket(delivery));
        // The sender is cloned and the clients guard released before anything
        // takes common again. Holding it across a give-back would take common
        // beneath clients, which is the forbidden direction.
        let sender = {
            let Ok(clients) = self.broker.registry.clients.lock() else {
                self.relinquish_outstanding_attempt(claim.token);
                return Some(false);
            };
            let sender = clients.get(&recipient).map(|senders| senders.ordered.clone());
            drop(clients);
            match sender {
                Some(sender) => sender,
                None => {
                    self.relinquish_outstanding_attempt(claim.token);
                    return Some(false);
                }
            }
        };

        // Written down before the handover: the record names the attempt it is
        // being made under, and the phase says the handover was begun. An
        // interruption from here leaves both, so the empty slot afterwards is
        // never read as "nothing was taken".
        // The custody phase moves with the record's. From here the attempt is
        // NOT an unused reservation: whatever happens next, it may have
        // reached the recipient.
        if let Some(custody) = self.terminal.attempt_custody.as_mut() {
            custody.phase = PrivateAttemptPhase::Dispatching;
        }
        let release = &mut self.terminal.settling[index];
        release.attempt = Some(claim.token);
        release.dispatch = PrivateDispatchPhase::Indeterminate;
        let Some(PrivatePendingDelivery::Capsule(capsule)) = release.pending.take() else {
            unreachable!("checked to be a capsule above")
        };
        // NOTHING FALLIBLE BETWEEN THE REFUSAL AND THE SLOT.
        match sender.try_send(capsule) {
            Ok(()) => {
                // Only the receipt obligation is kept. No replayable copy
                // stays here: the event is on the queue, and a second copy
                // would be a second event nobody asked for. The token moves
                // from outstanding onto the record it now serves.
                release.dispatch = PrivateDispatchPhase::Enqueued;
                if let Some(ticket) = admission {
                    // The admission this delivery went out under, so a later
                    // outcome can be checked against it rather than against a
                    // delivery id that may since have been handed out again.
                    release.record_admission(ticket);
                }
                self.terminal.attempt_custody = None;
                Some(true)
            }
            Err(std::sync::mpsc::TrySendError::Full(capsule)) => {
                // KNOWN NOT ENQUEUED. The same capsule is offered again later:
                // the bytes and the identity are the ones the release decided,
                // and nothing re-encodes or reselects anything.
                release.pending = Some(PrivatePendingDelivery::Capsule(capsule));
                release.dispatch = PrivateDispatchPhase::Pending;
                // KNOWN NOT ENQUEUED, so this attempt is an unused reservation
                // again and may be given back. The record stops naming it only
                // once the ledger confirms.
                self.mark_attempt_unplaced();
                self.relinquish_outstanding_attempt(claim.token);
                Some(false)
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(capsule)) => {
                release.pending = Some(PrivatePendingDelivery::Capsule(capsule));
                release.dispatch = PrivateDispatchPhase::Pending;
                self.mark_attempt_unplaced();
                self.relinquish_outstanding_attempt(claim.token);
                Some(false)
            }
        }
    }

    /// Finish one delivery attempt against the receipt its writer published.
    ///
    /// WHAT A RECEIPT PROVES IS DECIDED HERE, where the debt it belongs to is
    /// known. The receipt names a delivery; the debt is named by an
    /// incarnation; this record is the only thing holding both.
    ///
    /// ONLY AN ESTABLISHED FLUSH OR TERMINATION SETTLES THE RECIPIENT HALF.
    /// A write that failed and a wait that ran out are not proof that nobody
    /// received anything -- they are the absence of proof either way, and
    /// they finish the attempt with neither bit so the debt stays owed.
    ///
    /// AND THEY DO NOT AUTHORISE A REPLAY. A failed or timed-out write may
    /// have put part of the event on the wire, so the capsule is not returned
    /// to Pending; it is marked unrepeatable and never rebuilt. The debt may
    /// then never settle, which is the honest outcome and better than a
    /// duplicate nobody can detect.
    fn settle_one_receipt(&mut self) -> Option<PrivateReceiptStep> {
        // TAKE CUSTODY OF THE OUTCOME BEFORE ANYTHING CAN ERASE IT. Recovery
        // prunes a routing-finished ticket the moment an ordinary observer
        // consumes it, and that ticket is the only place the outcome lives.
        // A join that read it only when it was ready to settle could find the
        // attempt still out and its answer already gone.
        self.capture_available_outcomes();
        let found = self
            .terminal
            .settling
            .iter()
            .enumerate()
            .find_map(|(index, release)| {
                Some((index, release.attempt()?, release.outcome_seen()?))
            });
        let (index, token, outcome) = found?;
        let settlement = match outcome {
            // The writer established that the bytes went, or that the
            // recipient is gone. Both answer the recipient's half.
            XAuthorityInputDeliveryOutcome::Flushed
            | XAuthorityInputDeliveryOutcome::ClientDisconnected => {
                sophia_input_authority::SettlementBit {
                    native_reconciled: false,
                    recipient_settled: true,
                }
            }
            // Everything else is the absence of proof, including TimedOut and
            // WriteFailed. Neither bit, and the debt stays exactly as owed.
            _ => sophia_input_authority::SettlementBit::default(),
        };
        let answered = self.authority().under_common_as_origin(|authority, issuer| {
            authority.finish_attempt(issuer, token, settlement)
        });
        // The ledger's own answer about the whole debt, kept rather than
        // thrown away with the Result layers it arrived in.
        let debt_settled = matches!(answered, Ok(Ok(true)));
        if !matches!(answered, Ok(Ok(_))) {
            // THE LEDGER DID NOT ANSWER. Nothing here changes: the attempt is
            // still out, the outcome is still owned, and a later visit tries
            // again. Reporting this as a return would count a fact that has
            // not happened.
            return Some(PrivateReceiptStep::Unanswered);
        }
        if self
            .terminal
            .attempt_custody
            .is_some_and(|custody| custody.token == token)
        {
            self.terminal.attempt_custody = None;
        }
        let release = &mut self.terminal.settling[index];
        release.clear_attempt();
        if settlement.recipient_settled {
            return Some(PrivateReceiptStep::Settled { debt_settled });
        }
        // Possibly part-written, so never sent again.
        release.mark_unrepeatable();
        Some(PrivateReceiptStep::ReturnedUnsettled)
    }

    /// Bring every outcome recovery currently holds for this executor's
    /// dispatched releases into the records that own them.
    ///
    /// Checked against the admission each release was dispatched under, not
    /// against the delivery id alone. A pruned id handed out again carries a
    /// different admission, and an outcome published against that one answers
    /// somebody else's delivery.
    fn capture_available_outcomes(&mut self) {
        let recovery = &self.broker.registry.input_recovery;
        for release in &mut self.terminal.settling {
            if release.attempt().is_none() || release.outcome_seen().is_some() {
                continue;
            }
            let Some(delivery) = release.delivery() else {
                continue;
            };
            let Some(ticket) = recovery.ticket(delivery) else {
                continue;
            };
            if !release.admission_matches(&ticket) {
                continue;
            }
            let Some(receipt) = recovery.terminal_outcome(delivery) else {
                continue;
            };
            if receipt.client != release.reached().client() {
                continue;
            }
            release.record_outcome(receipt.outcome);
        }
    }

    /// Give one claimed attempt back with neither settlement bit.
    ///
    /// The debt stays exactly as owed as it was; this says only that the
    /// attempt did not happen. CONFIRMATION IS THE POINT: the token leaves
    /// outstanding storage and stops being named by its record only when the
    /// ledger has actually answered. A refused or unreadable give-back leaves
    /// both in place, so a later visit tries again rather than leaving the
    /// ledger holding a slot nobody remembers.
    fn mark_attempt_unplaced(&mut self) {
        if let Some(custody) = self.terminal.attempt_custody.as_mut() {
            custody.phase = PrivateAttemptPhase::Unplaced;
        }
    }

    fn relinquish_outstanding_attempt(
        &mut self,
        token: sophia_input_authority::AttemptToken,
    ) -> bool {
        let answered = self.authority().under_common_as_origin(|authority, issuer| {
            authority.finish_attempt(
                issuer,
                token,
                sophia_input_authority::SettlementBit::default(),
            )
        });
        // Ok(Ok(_)) is the ledger answering, whether or not it still held the
        // attempt. Anything else is no answer at all.
        if !matches!(answered, Ok(Ok(_))) {
            return false;
        }
        if self
            .terminal
            .attempt_custody
            .is_some_and(|custody| custody.token == token)
        {
            self.terminal.attempt_custody = None;
        }
        for release in &mut self.terminal.settling {
            if release.attempt() == Some(token) {
                release.clear_attempt();
            }
        }
        true
    }

    /// Spend one visit relinquishing an attempt whose give-back never landed.
    ///
    /// ONLY AN UNPLACED CLAIM. A claim whose handover began is not an unused
    /// reservation: its delivery may be on the recipient's queue, and giving
    /// the attempt back as unused would say a delivery that may have happened
    /// did not. That one waits for its receipt.
    fn relinquish_one_attempt(&mut self) -> Option<bool> {
        let custody = self.terminal.attempt_custody?;
        if custody.phase != PrivateAttemptPhase::Unplaced {
            return None;
        }
        Some(self.relinquish_outstanding_attempt(custody.token))
    }

    /// Whether a receipt is waiting to be answered against its debt.
    ///
    /// An outcome already owned counts, because recovery may have pruned the
    /// ticket it came from; so does one still sitting in recovery under this
    /// release's own admission.
    fn owes_receipt_settlement(&self) -> bool {
        let recovery = &self.broker.registry.input_recovery;
        self.terminal.settling.iter().any(|release| {
            release.attempt().is_some()
                && (release.outcome_seen().is_some()
                    || release.delivery().is_some_and(|delivery| {
                        recovery
                            .ticket(delivery)
                            .is_some_and(|ticket| release.admission_matches(&ticket))
                            && recovery.terminal_outcome(delivery).is_some()
                    }))
        })
    }

    /// Whether this executor holds return work of its own.
    ///
    /// Asked beside the release-derived work rather than through it. A held
    /// attempt is the ledger's slot, and when the record it was claimed for
    /// owes nothing further -- its proof recorded, its own attempt named --
    /// nothing else would make service look at this at all.
    fn owes_attempt_return(&self) -> bool {
        self.terminal
            .attempt_custody
            .is_some_and(|custody| custody.phase == PrivateAttemptPhase::Unplaced)
    }

    /// Whether any release is currently owed a recording visit.
    ///
    /// Asked before the step is charged, so an idle turn with nothing owed
    /// stays idle and costs nothing. Choosing the entry is still the visit's
    /// own job -- this only says whether there is one to choose.
    fn owes_native_recording(&self) -> bool {
        self.owes_receipt_settlement()
            || self.owes_attempt_return()
            || self.terminal.settling.iter().any(|release| {
                release.owes_native_recording() || release.owes_delivery_attempt()
            })
    }

    /// Spend one proof-recording visit on one chosen release.
    ///
    /// ONE ENTRY, ONE ATTEMPT. The cursor is retained across visits so they
    /// move through the releases that owe a recording rather than returning
    /// to whichever is first; a visit that swept the whole vector would do
    /// unbounded work and would make recording a side effect of something
    /// else rather than work the service can be asked for on its own.
    ///
    /// Returns None when nothing owes a recording, so an idle turn stays
    /// idle. The RELEASE IS NEVER REPLAYED here: the effect happened once and
    /// what is retried is only the recording of its proof.
    fn record_one_native(&mut self) -> Option<bool> {
        let settling = &mut self.terminal.settling;
        if settling.is_empty() {
            return None;
        }
        let cursor = &mut self.terminal.native_recording_cursor;
        for _ in 0..settling.len() {
            if *cursor >= settling.len() {
                *cursor = 0;
            }
            let chosen = *cursor;
            *cursor = cursor.saturating_add(1);
            if settling[chosen].owes_native_recording() {
                return Some(settling[chosen].record_native_once());
            }
        }
        None
    }
}
