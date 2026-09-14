// Closing a private instance and settling what it accepted.
//
// Split from construction and routing by subject: this is what happens when an
// instance stops, and what it hands on rather than destroys.

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Stop accepting, settle what can be settled, and keep the means to
    /// settle the rest.
    ///
    /// The returned handle retains the registry that accepted the work, so a
    /// caller can retry without holding a frontend or naming an authority. A
    /// report of bare operations would have been useless the moment this
    /// consumed self: the component able to answer them would have gone with
    /// it.
    pub fn shutdown(mut self) -> PrivateSettlement {
        self.terminal.lifecycle.close_all();
        let _lifecycle_progress = self.terminal.lifecycle.drive(NonZeroUsize::new(1).unwrap());
        self.settle_accepted()
    }

    /// Close and answer what was accepted, keeping what is still owed.
    ///
    /// Work that was routed and has not reached a terminal outcome is carried
    /// too. Draining only the admission queue would destroy those identities
    /// with the frontend, stranding their credits and losing any access to
    /// their completion -- including input that could still finish.
    fn settle_accepted(&mut self) -> PrivateSettlement {
        // Drop uses this path too. Closure is requested here without entering
        // common; explicit shutdown, retry, and durable drive perform cleanup.
        self.terminal.lifecycle.close_all();
        let origin = self.broker.registry.clone();
        if self.settled {
            return PrivateSettlement {
                origin,
                durable: self.durable.clone(),
                queue: Arc::clone(&self.admission.ready),
                pending: Vec::new(),
                outstanding: Vec::new(),
                queue_unreadable: false,
                settling: None,
                terminal: None,
            };
        }
        self.settled = true;
        // The parked operation never ran and carries no custody, so the
        // durable owner is exactly its home: it is work this instance accepted
        // and could not answer, which is what that owner exists for. Handed
        // over before the queue is closed, so it is not lost to an instance
        // that is going.
        if let Some((_, parked)) = self.parked.take() {
            // The completion record goes with the command, not beside it.
            // Handing the operation over while its record stayed Accepted left
            // two owners able to answer for one transaction: the cancellation
            // sweep below minted a second command from the record, and the
            // durable owner answered the first -- the same acknowledgement
            // twice for work that happened once.
            //
            // Taken by discard, which succeeds only for a record that has not
            // begun applying and is owed no receipt. A record that refuses is
            // one somebody else can still publish for, so the command is not
            // carried on as replayable work either.
            // Asked through the typed gate rather than by a boolean. A discard
            // that returns false covers an unreadable registry, a foreign
            // token, an absent record and several live phases at once, and
            // treating them alike loses the command and its credit whenever
            // the registry simply could not be read.
            match ownership_of(&origin, &parked) {
                // No live record can publish for it, so the command travels
                // and this owner answers it.
                SettlementOwnership::Ours => {
                    let carried = match parked {
                        PrivateOperation::Control(command, Some(_)) => {
                            PrivateOperation::Control(command, None)
                        }
                        other => other,
                    };
                    self.durable.take_one(&origin, carried);
                }
                // An established record owns the outcome. The command is not
                // carried on as replayable work, but its identity and the
                // credit it holds are, so the credit is freed when that record
                // retires rather than stranded.
                SettlementOwnership::Elsewhere => {
                    self.durable
                        .take_one_outstanding(&origin, PrivateIdentity::of(&parked));
                }
                // Nobody could say who owns it. Kept whole, credit and all:
                // an unreadable registry is not permission to publish, and it
                // is not evidence that something else will.
                SettlementOwnership::Unprovable => {
                    self.durable.take_one(&origin, parked);
                }
            }
        }
        self.parked_barrier = None;
        let stranded = match self.admission.close() {
            Ok(stranded) => stranded,
            Err(()) => {
                // The queue could not be opened, so nothing in it could be
                // recovered. The handle still carries the capability, so a
                // caller learns this from something that could have acted
                // rather than from a log line.
                self.failed = true;
                return PrivateSettlement {
                    origin,
                    durable: self.durable.clone(),
                    queue: Arc::clone(&self.admission.ready),
                    pending: Vec::new(),
                    outstanding: std::mem::take(&mut self.outstanding),
                    queue_unreadable: true,
                    settling: None,
                    terminal: Some(self.terminal.hand_over()),
                };
            }
        };
        // A credit belongs to work until that work is answered, wherever it
        // is answered. Releasing only on the drive path would leave credits
        // held against obligations that no longer exist.
        // Reclaim what has genuinely finished first, so work already answered
        // is not carried as though it were owed.
        self.reclaim_settled();
        let outstanding = std::mem::take(&mut self.outstanding);
        // Everything still in the queue is about to be answered or handed
        // back by the settlement below, so it already has an owner. Its
        // records are given up first, or the cancellation pass further down
        // would hand the same commands back a second time.
        for operation in &stranded {
            if let PrivateOperation::Control(_, Some(token)) = operation {
                self.completion.discard(*token);
            }
        }
        // Settled in place, through the same ownership gate every other
        // settlement path uses. The handover above frees each record, so the
        // gate passes; where it did not -- an unreadable registry, or a record
        // that had begun applying -- the gate is what stops this from
        // publishing an outcome someone else can still publish, and the credit
        // leaves with an identity rather than with a command that could be
        // sent again.
        let mut settlement = PrivateSettlement {
            origin,
            durable: self.durable.clone(),
            queue: Arc::clone(&self.admission.ready),
            pending: stranded,
            outstanding,
            queue_unreadable: false,
            settling: None,
            terminal: Some(self.terminal.hand_over()),
        };
        let _answered = settlement.settle_pending();
        // Records still unexecuted after the queue was answered belong to
        // commands a writer took and never ran: they left the queue, so
        // draining it did not reach them, and the instance is going. They are
        // carried out with a home rather than left in a registry nobody will
        // ask again.
        //
        // Their record is settled as it is taken, so the carried command has
        // no second owner that could publish an outcome for it. Commands
        // caught mid-application are not here: those stay in the registry,
        // which the returned settlement still reaches through its origin.
        let cancellation = self.completion.cancel_unfinished();
        // Ownership moves; it does not terminate. The identity leaves
        // `outstanding` in the same step that the command enters `pending`, so
        // exactly one owner holds the operation and exactly one credit is
        // released for it. Leaving the identity behind would let a watcher
        // read the record's absence as completion and release the credit here,
        // and settling `pending` would release it again.
        settlement.outstanding.retain(|identity| match identity {
            PrivateIdentity::Control {
                completion: Some(token),
                ..
            } => !cancellation
                .cancellable
                .iter()
                .any(|(cancelled, _)| cancelled == token),
            _ => true,
        });
        settlement.pending.extend(
            cancellation
                .cancellable
                .into_iter()
                .map(|(_, command)| PrivateOperation::Control(command, None)),
        );
        settlement
    }
}

#[cfg(unix)]
impl Drop for PrivateXServerFrontend {
    fn drop(&mut self) {
        // The fallback for an owner that never called shutdown. The handle
        // this produces is dropped immediately, and its own Drop makes one
        // final attempt with the capability still in hand.
        drop(self.settle_accepted());
        if self.failure_slot_held && !self.failed {
            // Closed without failing, so the slot belongs to whoever needs it
            // next rather than to an instance that has gone.
            self.durable.release_failure_slot();
            self.failure_slot_held = false;
        }
    }
}
