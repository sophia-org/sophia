// The source obligations a terminal visit answers.
//
// Split by subject from the ordered stepping beside it: that file is about
// moving one decided entry along its queue, this is about the debts a release
// leaves behind once its entry has gone -- a proof to record, a delivery to
// attempt, an attempt to give back. They change for different reasons, and
// the stepping is the part that has to stay readable end to end.

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Take one terminal step, if one is possible.
    ///
    /// At most one entry, chosen from work this instance already owns and
    /// moved only between places it owns, so nothing accepted is ever held
    /// outside the inventory. What comes back is a scalar: a report can be
    /// owned by a caller once its entry has been disposed of, the entry itself
    /// cannot.
    ///
    /// `start` is offered after the entry is chosen and before anything is
    /// observed, sent or guarded. An empty turn and a head nobody can describe
    /// cost nothing, because neither is a step; everything else is one, and a
    /// refusal leaves the work exactly where it already was.
    ///
    /// Advancing is not settling. A refusal moved into retained inventory
    /// consumed a real step and produced no report, and neither that nor a
    /// report itself says a recipient received anything.
    /// Claim one delivery attempt and hand its capsule to the recipient.
    ///
    /// ONE RELEASE, ONE ATTEMPT, and the two ends are joined here because
    /// nothing else holds both: the ledger names a debt by its incarnation
    /// and a receipt arrives naming a delivery. The ledger chooses which debt
    /// gets the turn; this only supplies the delivery for the one chosen.
    ///
    /// THE ORDER IS THE POINT.
    ///   1. the destination slot is prepared before anything is taken, so an
    ///      emission never leaves its hold with nowhere to be;
    ///   2. the attempt and the phase are written down BEFORE the handover,
    ///      because an attempt nobody recorded is one nothing can finish, and
    ///      a handover nobody marked is one an interruption makes invisible;
    ///   3. a refused queue returns the exact capsule and it goes straight
    ///      back into the slot with nothing fallible in between.
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
        // THE LEDGER SELECTS. Its cursor is the retained continuation: each
        // visit offers a different debt whether or not the last one could be
        // served, so nothing is permanently hidden by what sits at index zero.
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
        // PERSISTED BEFORE ANYTHING ELSE CAN FAIL. From here the token is
        // inventory-owned; no path below holds it only in a local, and an
        // unwind leaves it here to be relinquished rather than leaked.
        self.terminal.attempts_outstanding.push(claim.token);

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
                self.terminal
                    .attempts_outstanding
                    .retain(|token| *token != claim.token);
                Some(true)
            }
            Err(std::sync::mpsc::TrySendError::Full(capsule)) => {
                // KNOWN NOT ENQUEUED. The same capsule is offered again later:
                // the bytes and the identity are the ones the release decided,
                // and nothing re-encodes or reselects anything.
                release.pending = Some(PrivatePendingDelivery::Capsule(capsule));
                release.dispatch = PrivateDispatchPhase::Pending;
                // The record stops naming an attempt only once the ledger has
                // confirmed it back. Until then the token is still outstanding
                // and still this executor's to answer for.
                self.relinquish_outstanding_attempt(claim.token);
                Some(false)
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(capsule)) => {
                release.pending = Some(PrivatePendingDelivery::Capsule(capsule));
                release.dispatch = PrivateDispatchPhase::Pending;
                self.relinquish_outstanding_attempt(claim.token);
                Some(false)
            }
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
        self.terminal
            .attempts_outstanding
            .retain(|held| *held != token);
        for release in &mut self.terminal.settling {
            if release.attempt() == Some(token) {
                release.clear_attempt();
            }
        }
        true
    }

    /// Spend one visit relinquishing an attempt whose give-back never landed.
    fn relinquish_one_attempt(&mut self) -> Option<bool> {
        let token = *self.terminal.attempts_outstanding.first()?;
        Some(self.relinquish_outstanding_attempt(token))
    }

    /// Whether any release is currently owed a recording visit.
    ///
    /// Asked before the step is charged, so an idle turn with nothing owed
    /// stays idle and costs nothing. Choosing the entry is still the visit's
    /// own job -- this only says whether there is one to choose.
    fn owes_native_recording(&self) -> bool {
        self.terminal.settling.iter().any(|release| {
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
