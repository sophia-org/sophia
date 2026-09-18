// The source obligations a terminal visit answers.
//
// Split by subject from the ordered stepping beside it: that file is about
// moving one decided entry along its queue, this is about the debts a release
// leaves behind once its entry has gone -- a proof to record, a delivery to
// attempt, an attempt to give back. They change for different reasons, and
// the stepping is the part that has to stay readable end to end.

/// Which output a recipient's connection owes next.
///
/// ONE HEAD PER CONNECTION, ACROSS PRESS AND RELEASE ALIKE. Order is a
/// property of the recipient's own stream: a release that was decided before a
/// later press must reach that recipient first, and comparing only presses let
/// the later one overtake it.
///
/// UNFINISHED WORK STAYS IN THE COMPARISON. An event whose handover began and
/// never reported is exactly what must block everything behind it; excluding
/// it because it cannot advance removes the reason the others are waiting.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateOutputSite {
    /// The press custody on a hold record.
    HeldPress(usize),
    /// The press custody carried on a settling release.
    CarriedPress(usize),
    /// A settling release's own event.
    Release(usize),
    Transient(usize),
}

#[cfg(unix)]
impl PrivateXServerFrontend {
/// Wrap a press emission and stow it in the custody that owns it.
///
/// The slot is filled before anything is handed over, and an emission that
/// cannot be wrapped is retained there with its cause rather than dropped: it
/// is still the only copy of an event decided at a moment that has passed.
#[cfg(unix)]
fn stow_press_capsule(
    custody: &mut PrivateDeliveryCustody,
    emission: PrivateOrderedEmission,
    recovery: &InputRecovery,
    recipient: XServerFrontendClientId,
) {
    match XAuthorityOrderedDelivery::from_emission(emission) {
        Ok(mut capsule) => {
            if let Some(completion) = custody.completion.as_ref() {
                capsule.carry_finalizer(std::sync::Arc::new(finalizer_from_held(
                    recovery,
                    completion,
                    capsule.delivery(),
                    recipient,
                )));
            }
            custody.pending = Some(PrivatePendingDelivery::Capsule(capsule));
            custody.dispatch = PrivateDispatchPhase::Pending;
        }
        Err((cause, emission)) => {
            custody.pending = Some(PrivatePendingDelivery::Unwrapped { emission, cause });
            custody.dispatch = PrivateDispatchPhase::Unwrappable;
        }
    }
}

/// Hand one debt's event to its recipient, from the custody that owns it.
///
/// The order is the same wherever it is used: the slot is prepared before the
/// emission is taken, the phase is written down before the handover, and a
/// refused queue returns the exact capsule into the slot with nothing fallible
/// in between. Full is known-not-enqueued and is offered again unchanged;
/// nothing is ever re-encoded or reselected.
#[cfg(unix)]
fn dispatch_custody(
    custody: &mut PrivateDeliveryCustody,
    recipient: XServerFrontendClientId,
    recovery: &InputRecovery,
    clients: &Arc<Mutex<BTreeMap<XServerFrontendClientId, XServerFrontendClientRouteSenders>>>,
) -> bool {
    // THE PHASE AUTHORIZES, NOT THE SLOT. A capsule still sitting in a slot
    // whose phase says the handover may already have happened is not one to
    // send: an interruption after the write-ahead leaves exactly that, and
    // offering it again is a replay of bytes the recipient may already hold.
    if !custody.handover_permitted() {
        return false;
    }
    if !matches!(custody.pending, Some(PrivatePendingDelivery::Capsule(_))) {
        return false;
    }
    // THE ROW THAT IS CHECKED IS THE ROW THIS GOES TO. The endpoint the
    // capsule names is compared against the client-table entry under the same
    // guard the sender is cloned from, so there is no window between deciding
    // a row is the right one and taking the channel out of it. Validating one
    // lookup and sending through a second is the shape this avoids.
    //
    // A capsule whose endpoint is not this entry is NOT sent and NOT taken: it
    // stays in the custody that owns it, with its phase untouched, because a
    // replacement registration is not a reason to hand another registration's
    // work to it. What eventually becomes of such a capsule is retained
    // disposition, which is open work and deliberately not decided here.
    let endpoint = match custody.pending.as_ref() {
        Some(PrivatePendingDelivery::Capsule(capsule)) => capsule.endpoint().clone(),
        _ => return false,
    };
    let sender = {
        let Ok(guard) = clients.lock() else {
            return false;
        };
        let sender = guard.get(&recipient).and_then(|senders| {
            endpoint
                .is_entry(recipient, senders)
                .then(|| senders.ordered.clone())
        });
        drop(guard);
        match sender {
            Some(sender) => sender,
            None => return false,
        }
    };
    let _ = recovery;
    // ARMED BEFORE THE ADMISSION, and that order is the whole of it: locals
    // are destroyed in reverse, so a notice declared first is published last
    // -- after the gate is released -- on the way out of this function and on
    // an unwind through it alike. Declared the other way round, an unwind
    // would publish while the gate was still held.
    //
    // It is published whatever happens below, including on the refusals that
    // take nothing. A recheck request costs a waiter one look at its queue; a
    // notification that were only made on success would be the one not made
    // when a handover is accepted and then interrupted.
    let notify = sender.arm_wake();
    // ADMITTED BEFORE ANYTHING IS TAKEN. The sender above was captured under
    // the client table and the table is already released, so the capture alone
    // says nothing about whether this endpoint is still open. A refusal here
    // has taken nothing: the capsule stays where it is, with its phase
    // untouched, exactly as it does when the row is not this endpoint's.
    let Ok(admitted) = sender.admit() else {
        return false;
    };
    let _ = &notify;
    custody.dispatch = PrivateDispatchPhase::Indeterminate;
    let Some(PrivatePendingDelivery::Capsule(capsule)) = custody.pending.take() else {
        unreachable!("checked to be a capsule above")
    };
    // Held through the handover and through writing down what came back, so a
    // close cannot land between the two and report a fence over a handover
    // that had already happened.
    match admitted.try_send(capsule) {
        Ok(()) => {
            custody.dispatch = PrivateDispatchPhase::Enqueued;
            true
        }
        Err(std::sync::mpsc::TrySendError::Full(capsule))
        | Err(std::sync::mpsc::TrySendError::Disconnected(capsule)) => {
            custody.pending = Some(PrivatePendingDelivery::Capsule(capsule));
            custody.dispatch = PrivateDispatchPhase::Pending;
            false
        }
    }
}

    /// Whether the custody at this site may have a handover attempted for it.
    fn head_permits_handover(&self, site: PrivateOutputSite) -> bool {
        match site {
            PrivateOutputSite::Transient(index) => self.terminal.transients.records[index]
                .custody.handover_permitted(),
            PrivateOutputSite::HeldPress(index) => {
                self.terminal.holds[index].custody.handover_permitted()
            }
            PrivateOutputSite::CarriedPress(index) => self.terminal.settling[index]
                .press_custody
                .as_ref()
                .is_some_and(PrivateDeliveryCustody::handover_permitted),
            PrivateOutputSite::Release(index) => {
                self.terminal.settling[index].custody_handover_permitted()
            }
        }
    }

    /// The earliest unfinished output this recipient still owes.
    ///
    /// Found before asking whether it can advance, so a head that cannot move
    /// blocks what is behind it rather than being skipped.
    fn output_head(&self, recipient: XServerFrontendClientId) -> Option<(u64, PrivateOutputSite)> {
        let held = self
            .terminal
            .holds
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                record.reached.client() == recipient && record.custody.handover_unfinished()
            })
            .map(|(index, record)| (record.custody.order, PrivateOutputSite::HeldPress(index)));
        let carried = self
            .terminal
            .settling
            .iter()
            .enumerate()
            .filter(|(_, release)| release.reached().client() == recipient)
            .filter_map(|(index, release)| {
                release
                    .press_custody
                    .as_ref()
                    .filter(|custody| custody.handover_unfinished())
                    .map(|custody| (custody.order, PrivateOutputSite::CarriedPress(index)))
            });
        let releases = self
            .terminal
            .settling
            .iter()
            .enumerate()
            .filter(|(_, release)| {
                release.reached().client() == recipient && release.custody_handover_unfinished()
            })
            .map(|(index, release)| (release.custody_order(), PrivateOutputSite::Release(index)));
        let transients = self.terminal.transients.records.iter().enumerate()
            .filter(|(_, record)| record.source.reached().0 == recipient
                && record.custody.handover_unfinished())
            .map(|(index, record)| (record.custody.order, PrivateOutputSite::Transient(index)));
        held.chain(carried).chain(releases).chain(transients).min_by_key(|(order, _)| *order)
    }

    /// How far a connection sits after the one served last, cyclically.
    ///
    /// Zero would be the connection just served, so it comes last rather than
    /// first: a connection that has had its turn waits for the others.
    fn cyclic_position(&self, client: XServerFrontendClientId) -> u64 {
        let Some(last) = self.terminal.last_offered else {
            return client.raw();
        };
        client.raw().wrapping_sub(last.raw()).wrapping_sub(1)
    }

    /// The next output this visit may hand over, and whose it is.
    ///
    /// ONE PASS, AND ARBITRATION THAT SURVIVES THE VISIT. Connections are
    /// compared by how far they sit after the one served last, and the event
    /// stamp decides only within a connection -- so the winner is the lowest
    /// unfinished stamp of the connection whose turn it is, which is that
    /// connection's own head. No dedup list, no rescan per excluded
    /// connection, and unresolved heads stay in the comparison.
    ///
    /// Choosing the globally earliest output instead re-chose the same
    /// connection every visit whenever its head could not progress: a full
    /// queue restores the exact capsule and phase that selected it, so the
    /// next visit made the same choice and a connection with a later stamp
    /// never got a turn.
    ///
    /// THE STAMP IS NEVER CHANGED to achieve this. Fairness is whose turn it
    /// is; the stamp is the order its recipient must see.
    fn offered_head(&self) -> Option<(XServerFrontendClientId, PrivateOutputSite)> {
        let mut best: Option<(u64, u64, XServerFrontendClientId, PrivateOutputSite)> = None;
        let mut consider = |order: u64, client: XServerFrontendClientId, site, turn: u64| {
            if best.is_none_or(|(seen_turn, seen_order, _, _)| (turn, order) < (seen_turn, seen_order))
            {
                best = Some((turn, order, client, site));
            }
        };
        for (index, record) in self.terminal.holds.iter().enumerate() {
            if record.custody.handover_unfinished() {
                let client = record.reached.client();
                consider(
                    record.custody.order,
                    client,
                    PrivateOutputSite::HeldPress(index),
                    self.cyclic_position(client),
                );
            }
        }
        for (index, release) in self.terminal.settling.iter().enumerate() {
            let client = release.reached().client();
            let turn = self.cyclic_position(client);
            if let Some(custody) = release
                .press_custody
                .as_ref()
                .filter(|custody| custody.handover_unfinished())
            {
                consider(
                    custody.order,
                    client,
                    PrivateOutputSite::CarriedPress(index),
                    turn,
                );
            }
            if release.custody_handover_unfinished() {
                consider(
                    release.custody_order(),
                    client,
                    PrivateOutputSite::Release(index),
                    turn,
                );
            }
        }
        for (index, record) in self.terminal.transients.records.iter().enumerate() {
            if record.custody.handover_unfinished() {
                let client = record.source.reached().0;
                consider(record.custody.order, client, PrivateOutputSite::Transient(index),
                    self.cyclic_position(client));
            }
        }
        best.map(|(_, _, client, site)| (client, site))
    }

    /// Hand one recipient's next owed event to it.
    ///
    /// ONLY A HEAD MOVES. The offer is a single connection's earliest
    /// unfinished output, so a head that cannot advance blocks what is behind
    /// it on the same connection, which is what order means.
    ///
    /// Releases are heads too, but a release's handover belongs to the attempt
    /// path -- the ledger chooses which debt gets one -- so a release at the
    /// head is spent here without a send. What that costs is this visit; what
    /// it says is that the recipient is not free for anything behind it.
    ///
    /// ARBITRATED ACROSS RECIPIENTS. One connection whose head is stuck must
    /// not stop every other connection's output, so the offer moves on from
    /// the connection served last whether or not this visit hands anything
    /// over.
    fn dispatch_one_press(&mut self) -> Option<bool> {
        let (recipient, site) = self.offered_head()?;
        // THE TURN ADVANCES ON SELECTION, not on success. A queue that is full
        // restores the exact capsule and phase that chose this connection, so
        // advancing only when something was handed over would choose it again
        // on the next visit and never reach a connection behind it.
        self.terminal.last_offered = Some(recipient);

        // A head that belongs to the attempt path, or whose phase forbids a
        // handover, costs this visit and defers to a later round. Its turn has
        // been spent, so the next visit starts after it.
        if matches!(site, PrivateOutputSite::Release(_)) || !self.head_permits_handover(site) {
            return None;
        }

        let recovery = self.broker.registry.input_recovery.clone();
        let custody = match site {
            PrivateOutputSite::Transient(index) => {
                let record = &mut self.terminal.transients.records[index];
                if record.custody.pending.is_none() {
                    let Some(emission) = record.source.take_emission() else {
                        return Some(false);
                    };
                    Self::stow_press_capsule(&mut record.custody, emission, &recovery, recipient);
                }
                &mut record.custody
            }
            PrivateOutputSite::HeldPress(index) => {
                let record = &mut self.terminal.holds[index];
                if record.custody.pending.is_none() {
                    let taken = record
                        .native
                        .as_mut()
                        .and_then(PrivateNativeHold::take_press_emission);
                    let Some(emission) = taken else {
                        return Some(false);
                    };
                    Self::stow_press_capsule(
                        &mut record.custody,
                        emission,
                        &recovery,
                        recipient,
                    );
                }
                &mut record.custody
            }
            PrivateOutputSite::CarriedPress(index) => {
                let release = &mut self.terminal.settling[index];
                if release
                    .press_custody
                    .as_ref()
                    .is_some_and(|custody| custody.pending.is_none())
                {
                    let taken = release
                        .native_mut()
                        .and_then(PrivateNativeHold::take_press_emission);
                    let Some(emission) = taken else {
                        return Some(false);
                    };
                    Self::stow_press_capsule(
                        release.press_custody.as_mut().expect("selected by it"),
                        emission,
                        &recovery,
                        recipient,
                    );
                }
                release
                    .press_custody
                    .as_mut()
                    .expect("selected by its custody")
            }
            PrivateOutputSite::Release(_) => unreachable!("left to the attempt path"),
        };
        Some(Self::dispatch_custody(
            custody,
            recipient,
            &recovery,
            &self.broker.registry.clients,
        ))
    }

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

        // THE LEDGER'S CHOICE STANDS; what this decides is only whether this
        // executor can serve it now. A release that is not its connection's
        // output head must wait, and the attempt goes back rather than being
        // spent on an event that would arrive out of order.
        let servable = self
            .terminal
            .settling
            .iter()
            .position(|release| release.incarnation() == claim.hold)
            .filter(|index| self.terminal.settling[*index].owes_delivery_attempt())
            .filter(|index| {
                let release = &self.terminal.settling[*index];
                matches!(
                    self.output_head(release.reached().client()),
                    Some((_, PrivateOutputSite::Release(head))) if head == *index
                )
            });
        let Some(index) = servable else {
            // The ledger chose a debt this executor cannot serve. Its answer
            // stands; the token stays outstanding until the give-back is
            // confirmed.
            self.relinquish_outstanding_attempt(claim.token);
            return Some(false);
        };

        // BUILT FROM WHAT THIS RELEASE ALREADY HOLDS. Not fetched by delivery
        // id: that is the late acquisition a prune and a re-admission defeat,
        // and it would hand these bytes a finalizer for somebody else's
        // admission.
        let mut finalizer = self.terminal.settling[index].completion().map(|completion| {
            finalizer_from_held(
                &self.broker.registry.input_recovery,
                completion,
                self.terminal.settling[index]
                    .delivery()
                    .expect("a release with a completion has a delivery"),
                self.terminal.settling[index].reached().client(),
            )
        });
        // The destination slot is prepared before anything is taken from the
        // hold, so an emission never leaves its obligation with nowhere to be.
        if self.terminal.settling[index].custody.pending.is_none() {
            let taken = self.terminal.settling[index]
                .native_mut()
                .and_then(PrivateNativeHold::take_release_emission);
            let Some(emission) = taken else {
                self.relinquish_outstanding_attempt(claim.token);
                return Some(false);
            };
            let release = &mut self.terminal.settling[index];
            match XAuthorityOrderedDelivery::from_emission(emission) {
                Ok(mut capsule) => {
                    // The writer answers through a finalizer bound to this
                    // debt's own admission, so both are answering one
                    // admission rather than two lookups of one number -- and
                    // the writer's answer goes through the ledger rather than
                    // beside it.
                    if let Some(finalizer) = finalizer.take() {
                        capsule.carry_finalizer(std::sync::Arc::new(finalizer));
                    }
                    release.custody.pending = Some(PrivatePendingDelivery::Capsule(capsule));
                    release.custody.dispatch = PrivateDispatchPhase::Pending;
                }
                Err((cause, emission)) => {
                    // Retained rather than dropped. It cannot be wrapped now,
                    // and it is still the only copy of an event decided at a
                    // moment that has passed.
                    release.custody.pending = Some(PrivatePendingDelivery::Unwrapped { emission, cause });
                    release.custody.dispatch = PrivateDispatchPhase::Unwrappable;
                    self.relinquish_outstanding_attempt(claim.token);
                    return Some(false);
                }
            }
        }
        if !matches!(
            self.terminal.settling[index].custody.pending,
            Some(PrivatePendingDelivery::Capsule(_))
        ) {
            self.relinquish_outstanding_attempt(claim.token);
            return Some(false);
        }

        let recipient = self.terminal.settling[index].reached().client();
        // NO LOOKUP HERE. The handle was taken when this release was recorded
        // and is carried whole; fetching it again by delivery id would accept
        // whatever admission holds that number now, and on a retry it would
        // overwrite a correct handle with a replacement one.
        if self.terminal.settling[index].completion().is_none() {
            // Custody that is missing cannot be enqueued past. A delivery
            // handed over with no way to recognise its own answer is one whose
            // attempt nothing can ever finish.
            self.relinquish_outstanding_attempt(claim.token);
            return Some(false);
        }
        // THE ROW THAT IS CHECKED IS THE ROW THIS GOES TO, the same as for a
        // press. The endpoint the release's own capsule names is compared
        // against the client-table entry under the guard the sender is cloned
        // from, BEFORE the write-ahead and before the capsule is taken -- so a
        // release owed to a registration that is gone leaves its capsule and
        // its handover phase exactly as they were.
        //
        // The sender is cloned and the clients guard released before anything
        // takes common again. Holding it across a give-back would take common
        // beneath clients, which is the forbidden direction.
        let endpoint = match self.terminal.settling[index].custody.pending.as_ref() {
            Some(PrivatePendingDelivery::Capsule(capsule)) => capsule.endpoint().clone(),
            _ => {
                self.relinquish_outstanding_attempt(claim.token);
                return Some(false);
            }
        };
        let sender = {
            let Ok(clients) = self.broker.registry.clients.lock() else {
                self.relinquish_outstanding_attempt(claim.token);
                return Some(false);
            };
            let sender = clients.get(&recipient).and_then(|senders| {
                endpoint
                    .is_entry(recipient, senders)
                    .then(|| senders.ordered.clone())
            });
            drop(clients);
            match sender {
                Some(sender) => sender,
                None => {
                    // Nothing was written down and nothing was taken, so this
                    // attempt is still an unused reservation. It goes back
                    // through the confirmed give-back, with the record giving
                    // up its name for it only once the ledger agrees -- and the
                    // clients guard is already dropped, so common is taken
                    // beneath nothing.
                    self.relinquish_outstanding_attempt(claim.token);
                    return Some(false);
                }
            }
        };

        // ARMED BEFORE THE ADMISSION, so the reverse destruction order
        // publishes it after the gate is released -- on the way out and on an
        // unwind alike -- and after the give-back below, which takes common.
        // Nothing in publishing it reaches for common, the client table, the
        // payload, the output or the gate.
        let notify = sender.arm_wake();
        // ADMITTED BEFORE ANYTHING IS WRITTEN DOWN OR TAKEN. The sender was
        // captured under the client table, which is already released; the
        // capture alone does not say this endpoint is still open. A refusal
        // here has taken nothing and written nothing, so the attempt is still
        // an unused reservation and goes back the confirmed way -- with no
        // gate held, because none was obtained.
        let admitted = match sender.admit() {
            Ok(admitted) => admitted,
            Err(_refusal) => {
                self.relinquish_outstanding_attempt(claim.token);
                return Some(false);
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
        // Taken before the record is borrowed, and only for the acceptance
        // seam below: cloning the origin here ends its borrow immediately.
        #[cfg(all(test, unix))]
        let seam_origin = self.broker.registry.clone();
        let release = &mut self.terminal.settling[index];
        #[cfg(all(test, unix))]
        let seam_delivery = release.delivery();
        release.custody.attempt = Some(claim.token);
        release.custody.dispatch = PrivateDispatchPhase::Indeterminate;
        // The handle this release has carried since it was recorded is the one
        // that answers it. Nothing refreshes it here.
        debug_assert!(
            release.completion().is_some(),
            "custody was checked before the handover began"
        );
        let Some(PrivatePendingDelivery::Capsule(capsule)) = release.custody.pending.take() else {
            unreachable!("checked to be a capsule above")
        };
        // NOTHING FALLIBLE BETWEEN THE REFUSAL AND THE SLOT.
        //
        // The admission is held across the handover AND across writing down
        // what came back, so a close cannot land between them. The give-back
        // is deliberately not in here: it takes common, and taking common
        // beneath this gate would put every producer behind the ledger.
        let sent = admitted.try_send(capsule);
        // THE ONE INTERVAL NOBODY CAN DESCRIBE, made reachable to an
        // acceptance case and to nothing else. Between the handover returning
        // and the record of what it returned, an interruption leaves a
        // delivery whose fate is unknown; production cannot be asked to
        // produce that state on demand, so a case arms this exact origin and
        // delivery once and the call is empty for every other handover.
        #[cfg(all(test, unix))]
        routing_tests::m3_acceptance::after_ordered_handover(&seam_origin, seam_delivery);
        let handed_over = match sent {
            Ok(()) => {
                // Only the receipt obligation is kept. No replayable copy
                // stays here: the event is on the queue, and a second copy
                // would be a second event nobody asked for. The token moves
                // from outstanding onto the record it now serves.
                release.custody.dispatch = PrivateDispatchPhase::Enqueued;
                self.terminal.attempt_custody = None;
                true
            }
            Err(std::sync::mpsc::TrySendError::Full(capsule)) => {
                // KNOWN NOT ENQUEUED. The same capsule is offered again later:
                // the bytes and the identity are the ones the release decided,
                // and nothing re-encodes or reselects anything.
                release.custody.pending = Some(PrivatePendingDelivery::Capsule(capsule));
                release.custody.dispatch = PrivateDispatchPhase::Pending;
                // KNOWN NOT ENQUEUED, so this attempt is an unused reservation
                // again and may be given back.
                self.mark_attempt_unplaced();
                false
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(capsule)) => {
                release.custody.pending = Some(PrivatePendingDelivery::Capsule(capsule));
                release.custody.dispatch = PrivateDispatchPhase::Pending;
                self.mark_attempt_unplaced();
                false
            }
        };
        // The gate goes before the ledger does. The record stops naming the
        // attempt only once the ledger confirms, and that is common-side work.
        drop(admitted);
        if !handed_over {
            self.relinquish_outstanding_attempt(claim.token);
        }
        // Published here, after the ledger work and with no gate held. It says
        // only that this connection is worth looking at again.
        drop(notify);
        Some(handed_over)
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
        for release in &mut self.terminal.settling {
            if release.attempt().is_none() || release.outcome_seen().is_some() {
                continue;
            }
            let Some(receipt) = release.completion_answer() else {
                continue;
            };
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
        self.terminal.settling.iter().any(|release| {
            release.attempt().is_some()
                && (release.outcome_seen().is_some() || release.completion_answer().is_some())
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
            || self.owes_terminated_recipient()
            || self.terminal.owes_live_native_disposal()
            || self.terminal.transients.owes_visit()
            || (self.terminal.settling.len() > 1 && self.terminal.shared_activation.pending())
            || self.owes_attempt_return()
            || self
                .terminal
                .holds
                .iter()
                .any(PrivateHoldRecord::owes_press_handover)
            || self.terminal.settling.iter().any(|release| {
                release
                    .press_custody
                    .as_ref()
                    .is_some_and(PrivateDeliveryCustody::owes_handover)
            })
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

    fn join_shared_activations(&mut self) -> Option<PrivateDeliveryStep> {
        let terminal = &mut self.terminal;
        terminal.shared_activation.visit(&mut terminal.settling)
            .map(|(observed, joined)| {
                terminal.shared_activation_turn = false;
                terminal.native_class_debt = terminal.native_class_debt.saturating_add(1);
                PrivateDeliveryStep::SharedActivation { observed, joined }
            })
    }
}
