// What one connection's ordered output still owes after that connection ends.
//
// Split by subject from the settlement owner that stores it. That owner
// answers what happens to work an instance abandoned; this answers what one
// connection's ordered output IS when it is handed over, and what reserving a
// place for it costs before the connection is allowed to exist.


/// What became of the worker that was to serve a connection.
///
/// A FACT ABOUT THE WORKER, not about the payload or the record it sits in. A
/// worker can stop without poisoning anything, and a poisoned record is no
/// proof that anyone joined; keeping this separate is what stops one being
/// read as the other.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by reporting that is not attached yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrivateOrderedWorkerExit {
    /// NOBODY EVER STARTED ONE. There is nothing to join, and no join is
    /// manufactured to make the account look complete: a connection that was
    /// never served reaches retention by its own quiet path.
    ///
    /// The outcomes of a worker that did run -- returned, panicked, unknown --
    /// belong to the spawn, which is not landed. They are not written here in
    /// advance of the thing that would establish them.
    NeverStarted,
}

/// What is known about how a connection's ordered output reached its place.
///
/// THREE SEPARATE FACTS, kept apart because each is established by a different
/// thing and none implies another. What a close established is the producers'
/// side. What became of a worker is the consumer's. Whether the storage it was
/// taken from had been poisoned is neither: a panic can happen outside a visit
/// without poisoning anything, and poison can be found with no join having
/// happened at all.
///
/// CARRIED, NOT RECOMPUTED. It travels with the payload into the place,
/// because after that nothing can establish any of it: the gate is gone with
/// the registration, the worker is gone, and the storage it came from no
/// longer exists.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PrivateOrderedEvidence {
    /// What closing this connection's endpoint established, at teardown.
    ///
    /// `None` means no teardown outcome was recorded here -- not that the
    /// endpoint is open.
    fence: Option<PrivateHandoverFence>,
    /// What became of the worker that was to serve it.
    worker: PrivateOrderedWorkerExit,
    /// Whether the storage this payload was taken from had been poisoned.
    ///
    /// The work still moves -- refusing to move it would strand accepted work
    /// to make a point -- but moving it into a readable place does not make it
    /// readable, and the destination's own lock cannot carry a fact about the
    /// source's. Recording it here is what stops the transfer laundering it.
    source_poisoned: bool,
}

#[cfg(unix)]
impl PrivateOrderedEvidence {
    /// Nothing established yet: a connection being built.
    fn unstarted() -> Self {
        Self {
            fence: None,
            worker: PrivateOrderedWorkerExit::NeverStarted,
            source_poisoned: false,
        }
    }
}

/// A connection's ordered output, retained whole.
///
/// NOT UNPACKED, and not turned back into anything replayable. What is here
/// has applied, or may already be on a recipient's queue, so the only thing
/// carried is the right to go on answering for it.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Handed over by teardown; read by a driver that is not attached yet.
enum PrivateOrderedContinuation {
    /// A receiver that was minted and published before a serving owner could
    /// be built for it.
    ///
    /// The window is real: registration exposes this connection's ordered
    /// sender before the connection state, the recovery attachment and the
    /// binding have finished, so a capsule can be accepted into a queue whose
    /// owner does not exist yet. Discarding that queue because construction
    /// refused would drop an admission nobody ever answered for, so the exact
    /// pieces are kept with the refusal that stopped them.
    Setup {
        accepted: PrivateOrderedSetupCustody,
        refusal: X11OrderedServingRefusal,
        /// What is known about how this connection got here.
        evidence: PrivateOrderedEvidence,
        /// Capsules taken off that queue and still owed an answer.
        ///
        /// RECEIVED INTO CUSTODY, never probed away. Asking a channel whether
        /// it is finished means receiving from it, so anything that asked
        /// without keeping what it got would destroy accepted work to answer
        /// a question about it.
        retained: Vec<XAuthorityOrderedDelivery>,
        /// Whether that queue's producers are gone, as observed while
        /// receiving. Not asked, because asking consumes.
        drained: bool,
        /// Whether this connection's wire has been ended.
        ///
        /// A setup that got as far as binding holds the handle that can end
        /// it, and a connection nobody will ever serve is one whose recipient
        /// is otherwise left waiting for events that are not coming.
        ///
        /// Not independently witnessed: the same visit that ends the wire is
        /// the only thing that observes the queue finishing, so no control
        /// here separates the two. It would separate if an ending refused,
        /// which is the branch this host gives no honest way to reach. Kept
        /// because returning a place over an unended wire is the failure the
        /// review named.
        ended: bool,
        /// Why ending it refused, when it did.
        ///
        /// The same contract a serving owner keeps: a wire left unterminated
        /// says why, or whoever inherits it cannot.
        ending_refused: Option<std::io::ErrorKind>,
    },
    /// A serving owner, whole: its endpoint, receiver, queued contents,
    /// in-flight frame and offset, received-but-unclassified and foreign
    /// capsules, unanswered records with their own finalizers, the shared
    /// output, permission and stop handles, its independent shutdown handle,
    /// and whatever its close has established so far.
    Serving {
        /// The owner, in the home its preparation allocated for it.
        owner: PrivateServingHome,
        /// What is known about how this connection got here.
        ///
        /// THE SAME EVIDENCE, AND IT MUST NOT BE LOST IN THE CONVERSION. A
        /// serving owner answers for what it is holding; it says nothing about
        /// whether anything can still be handed to the connection, what became
        /// of the worker that was to serve it, or whether the storage it came
        /// from could be read.
        evidence: PrivateOrderedEvidence,
    },
}

/// What a connection had accepted when its setup refused.
///
/// HOW FAR IT GOT DECIDES WHAT THERE IS TO KEEP. A receiver that was minted
/// and published but never bound has its queue and nothing else. One that was
/// bound has the connection's output, its permission, its control counter, its
/// stop handle and -- the part that matters most -- the independent handle
/// that can still end the wire. Keeping only the queue in that case would
/// discard the one thing able to terminate a connection nobody will serve.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Handed over by teardown; read by a driver that is not attached yet.
enum PrivateOrderedSetupCustody {
    /// Published, never bound.
    Receiver(Box<XAuthorityOrderedReceiver>),
    /// Bound, never served.
    Transport(Box<XAuthorityOrderedTransport>),
}

/// One place in the continuation store.
///
/// RESERVED IS NOT FREE. A place that has been promised to a connection holds
/// nothing yet, and a store that could not tell those apart handed the same
/// place to two connections -- the second install would overwrite the first,
/// and the accepted work in it would go with no record that it had existed.
#[cfg(unix)]
enum PrivateOrderedContinuationPlace {
    /// Nobody holds this place.
    Free,
    /// Promised to a connection, with its record already made and empty.
    ///
    /// THE RECORD EXISTS FROM THE RESERVATION, not from the hand-over. Making
    /// it while installing put an allocation inside the one interval that must
    /// not contain one: between taking a connection's work out of its source
    /// and putting it somewhere, where anything that can fail loses it. The
    /// aggregate's place and the record are different storage, and reserving
    /// has to make both.
    ///
    /// SEPARATELY OWNED, too. Driving a continuation means calling into its
    /// close, which takes this connection's output and its finalizers; doing
    /// that under the aggregate lock would put the whole store behind one
    /// connection's write, and would take common beneath settlement. The
    /// record has its own lock, so the aggregate one is held only long enough
    /// to find it.
    Taken(Arc<Mutex<Option<PrivateOrderedContinuation>>>),
}

/// A reserved place for one connection's ordered continuation.
///
/// TAKEN BEFORE THE CONNECTION IS EXPOSED, because a connection that cannot be
/// handed over is one whose accepted work has nowhere to go the moment
/// anything fails. A refused reservation leaves the connection unbuilt.
///
/// NEITHER COPY NOR CLONE. It is one place, held by one connection for the
/// whole of its ownership interval, and it is not given back because setup
/// failed, because the client went, or because the owner moved into retained
/// storage -- retained work still occupies the place it was promised. It comes
/// back when that work is finished and its disposition established, and the
/// only other way out is an explicit transfer to somewhere already reserved.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Handed over by teardown; read by a driver that is not attached yet.
struct PrivateOrderedContinuationSlot {
    /// The store this place belongs to, held as an owner.
    ///
    /// AN EXTERNAL RESERVATION IS A LEGITIMATE OWNER. This slot lives outside
    /// the store, in the registration of a connection that has been exposed
    /// and can already have work accepted for it. Nothing inside the store
    /// reaches it, so it is on no ring: it is one end of a lifetime, and the
    /// store must outlive it because there is a place in there this connection
    /// still has to dispose of.
    ///
    /// AND NOTHING ELSE ENFORCES THAT. The constructor takes the store by
    /// reference, but a reference parameter does not bind the instance or the
    /// registrations it hands out to the caller's binding, and the caller may
    /// drop its own holder while an exposed connection is still live. Held
    /// here, the connection carries its own guarantee, which is the only place
    /// it can be carried from.
    ///
    /// AN INTERNAL CREDIT IS NOT THIS SHAPE. A place-reference that lives
    /// INSIDE the store -- what a reaper record will need in order to name the
    /// place it is finishing -- closes a ring: store, record, credit, store.
    /// That holder must not be an owner, and converting it is a separate step
    /// with its own controls. Nothing here makes that conversion, and this
    /// field must not be copied into a holder the store itself keeps.
    owner: PrivateSettlementOwner,
    /// Which reserved place this is. The storage exists from the moment the
    /// slot does, so installing into it allocates nothing.
    index: usize,
    /// Whether this slot still has to be disposed of.
    ///
    /// Cleared by whichever disposal actually happens, so the fallback in Drop
    /// cannot transfer or account for the same place twice.
    armed: bool,
}

/// Where a hand-over actually put the work.
///
/// SAID, NOT ASSUMED. Two of these three leave the source holding what it came
/// with, and a caller that treated every return as an installation would be
/// asserting a premise rather than reading a result. It decides custody, so
/// discarding it is discarding the answer to where accepted work went.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
enum PrivateContinuationInstall {
    /// Into the place reserved for it. The source is empty.
    Installed,
    /// The place was not there. Marked abandoned; the source keeps its work.
    NoPlace,
    /// The source held nothing. Marked abandoned.
    NothingHandedOver,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Handed over by teardown; read by a driver that is not attached yet.
impl PrivateOrderedContinuationSlot {
    /// Put this connection's continuation in the place reserved for it.
    ///
    /// TAKEN FROM A SOURCE-OWNED SLOT, NOT BY VALUE. A continuation passed by
    /// value is in a stack frame from the caller's expression until this
    /// reaches its destination, and an unwind anywhere in between -- acquiring
    /// the store, above all -- drops accepted work while the reservation for
    /// it survives. It stays in the caller's slot until the destination is
    /// held, and is taken out only once there is somewhere for it to go.
    ///
    /// The storage itself was made when the place was reserved, so between the
    /// take and the installation there is nothing fallible, no allocation and
    /// no callback.
    ///
    /// An unreadable owner does not make this optional. The work has been
    /// accepted and the place is this connection's; skipping the move would
    /// drop it, so the poisoned guard is used exactly as every other
    /// already-accepted move here uses it.
    ///
    /// CONSUMES THE CAPABILITY. A slot that stayed usable after installing had
    /// only a debug assertion between a second call and overwriting held work,
    /// and that protection is not there in a release build.
    fn install(
        mut self,
        source: &mut Option<PrivateOrderedContinuation>,
    ) -> PrivateContinuationInstall {
        // The record was made when this place was reserved. Finding it is a
        // reference count, not an allocation, and it happens while the work is
        // still the caller's.
        let record = {
            let held = self.owner.records_even_if_poisoned();
            match held.continuations.get(self.index) {
                Some(PrivateOrderedContinuationPlace::Taken(record)) => record.clone(),
                _ => {
                    // No place to install into. Nothing is taken, so the work
                    // stays with its source and this is reported rather than
                    // quietly counted as done.
                    drop(held);
                    self.abandon();
                    return PrivateContinuationInstall::NoPlace;
                }
            }
        };
        // THE DESTINATION IS HELD FIRST. From here to the assignment there is
        // no allocation, no callback and nothing that can fail.
        //
        // No control here discriminates this order: once install is entered
        // the source is inside it, and the interval between a take and an
        // assignment is not observable from outside without reaching in. What
        // the control beside it establishes is that a hand-over WAITS for its
        // destination rather than completing; the ordering itself rests on
        // this being the only take, textually after the acquisition.
        let mut destination = record
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(continuation) = source.take() else {
            // Nothing was handed over. The place is not installed into and not
            // returned either -- no holder and no driver remains for it -- so
            // it is recorded as what it is rather than left looking taken by
            // someone who will come back for it.
            drop(destination);
            self.abandon();
            return PrivateContinuationInstall::NothingHandedOver;
        };
        *destination = Some(continuation);
        self.armed = false;
        PrivateContinuationInstall::Installed
    }

    /// Give up this place without disposing of it, and say so.
    fn abandon(&mut self) {
        let mut held = self.owner.records_even_if_poisoned();
        held.continuations_abandoned = held.continuations_abandoned.saturating_add(1);
        self.armed = false;
    }

    /// Give the place back, for a connection that was never exposed.
    ///
    /// A RESERVATION THAT PUBLISHED NOTHING IS NOT RETAINED WORK. Nothing could
    /// have been accepted for a connection with no row and no reachable queue,
    /// so there is nothing to account for and the capacity is free. Holding it
    /// would spend a place on a connection that never existed.
    ///
    /// Only for a KNOWN pre-publication failure. A disposition that is merely
    /// unknown keeps its place.
    fn relinquish_unexposed(self) {
        self.give_back();
    }

    /// Give the place back, for a connection that finished owing nothing.
    ///
    /// Only for that. A slot returned while its owner still holds unanswered
    /// admissions, foreign capsules or a wire whose ending was never
    /// established would be capacity handed out against work that still
    /// exists.
    fn finish(self) {
        self.give_back();
    }

    fn give_back(mut self) {
        let mut held = self.owner.records_even_if_poisoned();
        // The place is checked, NOT the record's contents: reading a record
        // here would take one beneath the aggregate, which is the order
        // driving relies on being the other way round. A debug assertion is
        // not a reason to close a lock cycle.
        debug_assert!(
            matches!(
                &held.continuations[self.index],
                PrivateOrderedContinuationPlace::Taken(_)
            ),
            "a finished slot is still this slot's place"
        );
        held.continuation_slots = held.continuation_slots.saturating_sub(1);
        held.continuations[self.index] = PrivateOrderedContinuationPlace::Free;
        self.armed = false;
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Handed over by teardown; read by a driver that is not attached yet.
impl PrivateOrderedContinuation {
    /// The queue this continuation still holds, whichever case it is.
    ///
    /// A connection that never got a serving owner still has whatever was
    /// accepted into its queue before it failed, and that is the thing a
    /// retained Setup case exists to keep.
    fn queue(&self) -> &Receiver<XAuthorityOrderedDelivery> {
        match self {
            Self::Setup {
                accepted: PrivateOrderedSetupCustody::Receiver(ordered),
                ..
            } => ordered,
            Self::Setup {
                accepted: PrivateOrderedSetupCustody::Transport(transport),
                ..
            } => &transport.ordered,
            Self::Serving { owner, .. } => &owner.queue,
        }
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Handed over by teardown; read by a driver that is not attached yet.
impl PrivateOrderedContinuation {
    /// One bounded step of whatever this continuation still owes.
    ///
    /// A serving owner closes. A setup that never got one is not idle either:
    /// it ends its wire if it has the handle for one, takes at most one
    /// capsule off its queue into retained custody, and records whether that
    /// queue's producers have gone. The predicate reads the facts recorded by
    /// those operations: receiving is what establishes that a channel is
    /// finished, and a shutdown's own result is what establishes an ending.
    fn visit(&mut self) {
        if let Self::Setup {
            accepted,
            retained,
            ended,
            ending_refused,
            ..
        } = self
        {
            // A connection nobody will serve still has a recipient waiting.
            // Ending it is the one disposition this case can perform -- when
            // it has anything to perform it with.
            if !*ended {
                *ended = match accepted {
                    // NOT ENDED. HAVING NO HANDLE IS NOT HAVING ENDED
                    // SOMETHING. A receiver alone carries no way to reach the
                    // connection, so nothing here has touched that socket and
                    // this record knows nothing about it -- not that it is
                    // open, and not that it closed. Whether it did depends on
                    // who else holds a descriptor for it, which is not
                    // knowable from here.
                    //
                    // Recording `ended` because there is nothing to end with
                    // read as an established fact, let `settled` agree, and
                    // handed the place back over a connection whose state
                    // nobody had established, with accepted work still on its
                    // queue.
                    //
                    // So this stays false, and the place stays held. That is
                    // the honest outcome of a connection whose binding refused:
                    // there is work nobody can deliver and a wire nobody here
                    // can close, and saying so is the only thing left to do
                    // about it.
                    PrivateOrderedSetupCustody::Receiver(_) => false,
                    PrivateOrderedSetupCustody::Transport(transport) => {
                        match transport.shutdown.shutdown(Shutdown::Both) {
                            Ok(()) => true,
                            Err(error) if error.kind() == std::io::ErrorKind::NotConnected => true,
                            Err(error) => {
                                *ending_refused = Some(error.kind());
                                false
                            }
                        }
                    }
                };

            }
            // ROOM BEFORE CUSTODY, as everywhere else: what is already held is
            // bounded by what the queue itself could hold.
            let bound = match accepted {
                PrivateOrderedSetupCustody::Receiver(ordered) => ordered.capacity(),
                PrivateOrderedSetupCustody::Transport(transport) => {
                    transport.ordered.capacity()
                }
            };
            if retained.len() >= bound {
                return;
            }
            // ROOM BEFORE THE RECEIVE, not after it. Pushing into a store that
            // then has to grow would allocate while holding a capsule taken
            // out of its queue, which is the interval nothing may fail in.
            if retained.capacity() < bound && retained.try_reserve(bound).is_err() {
                return;
            }
            match self.queue().try_recv() {
                Ok(capsule) => {
                    let Self::Setup { retained, .. } = self else {
                        unreachable!("matched above")
                    };
                    retained.push(capsule);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    let Self::Setup { drained, .. } = self else {
                        unreachable!("matched above")
                    };
                    *drained = true;
                }
            }
            return;
        }
        let Self::Serving { owner, .. } = self else {
            return;
        };
        // ASKED AGAIN WHILE IT IS NOT ESTABLISHED. A close whose shutdown was
        // refused left this record with serving excluded, a wire possibly
        // still carrying bytes, and nothing that would ever try again: the
        // guard only asked when no close existed, so the first refusal was the
        // last attempt. A retained connection that nobody retries is one whose
        // recipient waits for an ending that is not coming.
        //
        // WHAT A RETRY IS NOT. It is not a new close: the original cause and
        // identity are the ones already recorded, and asking again does not
        // replace them. It does not reset the attempt count, so the effort
        // this owner may spend locally is the effort it had left. It does not
        // unbar the wire, replay anything, or publish anything -- an ending is
        // offered only once the termination is established, which is asked
        // for separately and not assumed from having tried.
        //
        // Nothing here bounds how long an attempt takes. What it bounds is how
        // many are made from this owner.
        if owner
            .begin_close(X11OrderedCloseCause::SupervisorStopped)
            .is_err()
        {
            // Either it refused again or there are no attempts left. Both
            // leave termination unestablished, so nothing is offered and the
            // place stays taken; which of the two it was is on the record.
            return;
        }
        owner.advance_close(XByteOrder::LittleEndian, 0);
    }

    /// ASKS NOTHING OF THE QUEUE. Receiving is the only way to question a
    /// channel, so a predicate that questioned one would consume what was
    /// waiting and report on work it had just destroyed. Everything here was
    /// recorded by a visit that was receiving anyway.
    fn settled(&self) -> bool {
        match self {
            Self::Setup {
                retained,
                drained,
                ended,
                evidence,
                ..
            } => {
                // AN ESTABLISHED CLOSURE IS PART OF BEING SETTLED, and it is
                // not what tells a finished channel from a quiet one. `drained`
                // already does that: it is set from Disconnected, which means
                // every sender is gone and no later send is possible. Nothing
                // here needs the fence for that.
                //
                // What the fence adds is evidence about the OTHER side of a
                // handover. A holder that panicked inside the gate may have
                // left one half-answered, and no amount of quiet on this queue
                // speaks to that. So an unestablished closure keeps the record
                // open.
                //
                // And an established one is not a settlement of everything a
                // producer holds. It says this endpoint admitted nothing
                // further and nothing was interrupted in the act; what a
                // producer still owes elsewhere is its own accounting, and no
                // receipt or termination is inferred from a fence.
                matches!(
                    evidence.fence,
                    Some(
                        PrivateHandoverFence::Established
                            | PrivateHandoverFence::AlreadyEstablished
                    )
                ) && *drained
                    && *ended
                    && retained.is_empty()
            }
            Self::Serving { owner, evidence } => {
                // THE SAME CLOSURE CONDITION AS A SETUP RECORD. An owner that
                // has terminated has answered for what it held; the closure is
                // the separate fact that the handover which produced this
                // connection's work was not left half-answered. One does not
                // stand in for the other, and a conversion that dropped the
                // closure would let the owner's own termination speak for it.
                matches!(
                    evidence.fence,
                    Some(
                        PrivateHandoverFence::Established
                            | PrivateHandoverFence::AlreadyEstablished
                    )
                ) && owner.retained_unanswered().is_empty()
                    && owner.retained_foreign().is_empty()
                    && owner.in_flight().is_none()
                    && owner.refused().is_none()
                    && !owner.unterminated
                    && owner.closing().is_some_and(|closing| {
                        closing.termination == X11OrderedTermination::Established
                            && closing.drained
                    })
            }
        }
    }
}

/// A settlement store a holder may use but does not own.
///
/// See [`PrivateSettlementOwner::settlement_ref`] for why this is the only
/// shape a registry may hold one in.
#[cfg(unix)]
#[derive(Clone)]
struct PrivateSettlementRef {
    inner: std::sync::Weak<Mutex<AbandonedSettlements>>,
}

#[cfg(unix)]
impl PrivateSettlementRef {
    /// The store, if it is still there.
    ///
    /// A store that has gone is not an empty store: nothing can be handed to
    /// it, and a caller that needs one must refuse rather than carry on
    /// without it.
    fn owner(&self) -> Option<PrivateSettlementOwner> {
        self.inner
            .upgrade()
            .map(|inner| PrivateSettlementOwner { inner })
    }
}

#[cfg(unix)]
impl PrivateSettlementOwner {
    /// A handle to this store that does not keep it alive.
    ///
    /// WEAK BY CONSTRUCTION, and it has to be. The store retains records that
    /// hold registries: an instance that ends owing a hold hands its terminal
    /// inventory over, and that inventory keeps the registry it must act
    /// through in order to answer. A registry that then held the store
    /// strongly would close the ring -- store, retained inventory, registry,
    /// store -- and nothing in it would ever drop. The obligations would stay
    /// readable forever, which reads as "still owed" rather than "leaked", so
    /// the leak would present as an instance that never finishes settling.
    fn settlement_ref(&self) -> PrivateSettlementRef {
        PrivateSettlementRef {
            inner: Arc::downgrade(&self.inner),
        }
    }

    /// Declare how many connections may hold a place at once.
    ///
    /// DECLARED ONCE, AND NEVER RESET OVER RETAINED WORK. This store outlives
    /// the instance that configured it -- that is what durable means -- so a
    /// later instance arriving with its own client limit finds places already
    /// held against the first one. Moving the number under them would either
    /// strand work above a smaller bound or hand out places a departed
    /// instance already accounted for. The bound in force wins and is reported
    /// back, so a caller can tell whether the number it asked for is the
    /// number it got.
    ///
    /// Refuses only for an unreadable store, which is not the same answer as
    /// a bound of nothing.
    fn declare_connection_bound(&self, connections: NonZeroUsize) -> Option<usize> {
        let Ok(mut held) = self.inner.lock() else {
            return None;
        };
        if held.continuation_bound_declared {
            return Some(held.continuation_capacity);
        }
        // The OUTER storage is made here, so a reservation takes an index in a
        // vector that already has room rather than growing one. The record at
        // that index is allocated by the reservation itself, before exposure
        // and not while holding custody.
        held.continuations.reserve(connections.get());
        held.continuation_capacity = connections.get();
        held.continuation_bound_declared = true;
        Some(connections.get())
    }

    /// Take the place one connection's ordered continuation will need.
    ///
    /// BEFORE THAT CONNECTION'S ROW IS PUBLISHED. The channel is already made
    /// by then -- the sender exists -- but nothing can reach it until the row
    /// is in, and from that moment a capsule can be accepted into the queue
    /// and the connection has work that must be able to go somewhere.
    /// Publication is the boundary, not the sender's existence, and reserving
    /// after it would be finding out too late.
    ///
    /// Takes an index in the outer storage the declared bound already made.
    /// The record itself is allocated here -- before exposure, which is what
    /// matters -- rather than later while holding custody.
    fn reserve_ordered_continuation(
        &self,
    ) -> Result<PrivateOrderedContinuationSlot, AdmissionRefusal> {
        // An unreachable owner and a full one are different answers, for the
        // same reason as every other reservation here.
        let Ok(mut held) = self.inner.lock() else {
            return Err(AdmissionRefusal::Unavailable);
        };
        if held.continuation_slots >= held.continuation_capacity {
            return Err(AdmissionRefusal::Saturated);
        }
        // A FREE place, not merely an empty one. A place already promised to
        // another connection holds nothing yet, and taking it again would give
        // two connections the same destination.
        let index = match held
            .continuations
            .iter()
            .position(|place| matches!(place, PrivateOrderedContinuationPlace::Free))
        {
            Some(index) => index,
            None => {
                held.continuations
                    .push(PrivateOrderedContinuationPlace::Free);
                held.continuations.len() - 1
            }
        };
        // The record is made HERE, before this connection is exposed, so the
        // hand-over later is a move into storage that already exists.
        held.continuations[index] =
            PrivateOrderedContinuationPlace::Taken(Arc::new(Mutex::new(None)));
        held.continuation_slots = held.continuation_slots.saturating_add(1);
        drop(held);
        Ok(PrivateOrderedContinuationSlot {
            // An owner: this connection is about to be exposed -- the row is
            // published after this returns -- and the place it will have to
            // dispose of is in there.
            owner: self.clone(),
            index,
            armed: true,
        })
    }

    /// How many places are taken, live and retained together.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn continuations_reserved(&self) -> Option<usize> {
        self.inner.lock().ok().map(|held| held.continuation_slots)
    }

    /// How many continuations are actually stored here.
    ///
    /// HANDLES ARE COPIED UNDER THE AGGREGATE AND READ AFTER IT IS RELEASED.
    /// Taking a record beneath the aggregate is the reverse of the order
    /// driving uses -- a driver holds a record and may enter settlement -- so a
    /// reader that did it would close the cycle from the other side.
    ///
    /// AN UNREADABLE RECORD IS NOT AN ABSENT ONE. Counting a poisoned record
    /// as empty publishes a zero for an obligation that is still owned and
    /// still unanswered, which is the one answer a caller must not be given.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn continuations_retained(&self) -> Option<usize> {
        let records: Vec<_> = {
            let held = self.inner.lock().ok()?;
            held.continuations
                .iter()
                .filter_map(|place| match place {
                    PrivateOrderedContinuationPlace::Taken(record) => Some(record.clone()),
                    PrivateOrderedContinuationPlace::Free => None,
                })
                .collect()
        };
        let mut retained = 0usize;
        for record in records {
            let Ok(record) = record.lock() else {
                return None;
            };
            retained += usize::from(record.is_some());
        }
        Some(retained)
    }

    /// How many places went with a holder that disposed of neither.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn continuations_abandoned(&self) -> Option<usize> {
        self.inner.lock().ok().map(|held| held.continuations_abandoned)
    }

    /// Give one bounded visit to each retained continuation in turn.
    ///
    /// FAIR, so one connection that cannot progress does not consume every
    /// visit. The cursor is retained, so the next call starts after the one
    /// served last rather than at the front -- a blocked record at the front
    /// would otherwise take the whole budget every time.
    ///
    /// A PLACE COMES BACK ONLY WHEN ITS WORK IS GONE. Not when its queue falls
    /// quiet -- producers may still hold senders -- and not when it drains with
    /// an admission still unanswered, a capsule belonging to elsewhere, or a
    /// wire whose ending was never established.
    #[cfg_attr(not(test), allow(dead_code))] // Handed over by teardown; read by a driver that is not attached yet.
    fn drive_ordered_continuations(&self, visits: usize) -> usize {
        let mut driven = 0usize;
        for _ in 0..visits {
            let (index, record) = {
                let mut held = self.records_even_if_poisoned();
                let places = held.continuations.len();
                if places == 0 {
                    return driven;
                }
                let mut found = None;
                for step in 0..places {
                    let index = (held.continuation_cursor + step) % places;
                    if let PrivateOrderedContinuationPlace::Taken(record) =
                        &held.continuations[index]
                    {
                        found = Some((index, record.clone()));
                        break;
                    }
                }
                held.continuation_cursor = found
                    .as_ref()
                    .map_or(held.continuation_cursor, |(index, _)| (index + 1) % places);
                match found {
                    Some(found) => found,
                    None => return driven,
                }
            };
            // Driven with the store released.
            let settled = {
                let mut record = record
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let Some(continuation) = record.as_mut() else {
                    // Reserved but never handed anything over. Nothing to
                    // drive, and not this drive's business to reclaim.
                    continue;
                };
                continuation.visit();
                continuation.settled()
            };
            driven += 1;
            if settled {
                self.return_ordered_continuation(index, &record);
            }
        }
        driven
    }

    /// Give a place back, once the work in it is gone.
    #[cfg_attr(not(test), allow(dead_code))] // Handed over by teardown; read by a driver that is not attached yet.
    fn return_ordered_continuation(
        &self,
        index: usize,
        record: &Arc<Mutex<Option<PrivateOrderedContinuation>>>,
    ) {
        let mut held = self.records_even_if_poisoned();
        let PrivateOrderedContinuationPlace::Taken(place) = &held.continuations[index] else {
            return;
        };
        if !Arc::ptr_eq(place, record) {
            // The place has moved on to another connection since this visit
            // began. Returning it now would take somebody else's.
            //
            // Reached by a control that stages the interleave at this API --
            // a place returned, reserved again, then given a stale return
            // against the record it used to hold. That is not an observed
            // concurrent race, and it says nothing about acquisition being
            // bounded; what it establishes is that this comparison is what
            // stops a successor's place being freed.
            return;
        }
        held.continuations[index] = PrivateOrderedContinuationPlace::Free;
        held.continuation_slots = held.continuation_slots.saturating_sub(1);
        // A RETURNED PLACE LEAVES NO HOLDER BEHIND NAMING IT. The next
        // connection to reserve takes this index, and a holder still pointing
        // at it would be reading that connection's queue and could free its
        // place. Taken out here and dropped after the store is released: a
        // credit's disposal takes the store, and dropping one under this guard
        // would be this thread waiting for itself.
        let retired = Self::retire_holder_for(&mut held, index, record);
        drop(held);
        drop(retired);
    }

    /// Borrow one retained continuation, if the place holds one.
    ///
    /// BORROWED, NOT TAKEN OUT. Acting on a continuation means calling into
    /// its close, which takes this connection's output and its finalizers;
    /// doing that with the record in a local would put retained work in a
    /// stack frame across exactly the calls that can unwind.
    ///
    /// THE STORE IS RELEASED FIRST. Its order is settlement before anything a
    /// close touches, so holding it while one connection waits on its output
    /// would invert that and put every other retained connection behind that
    /// write. The record has its own lock, and the store is held only long
    /// enough to find it.
    #[cfg_attr(not(test), allow(dead_code))] // Handed over by teardown; read by a driver that is not attached yet.
    fn with_ordered_continuation<R>(
        &self,
        index: usize,
        act: impl FnOnce(&mut PrivateOrderedContinuation) -> R,
    ) -> Option<R> {
        // The aggregate lock is held only to find the record's own handle.
        let record = {
            let held = self.records_even_if_poisoned();
            let PrivateOrderedContinuationPlace::Taken(record) = held.continuations.get(index)?
            else {
                return None;
            };
            record.clone()
        };
        // RELEASED BEFORE ANYTHING IS DRIVEN. The order is settlement before
        // whatever a close touches, so holding the store while one connection
        // waits on its output would invert it and put every other retained
        // connection behind that write. The record is borrowed in its own
        // storage instead -- it is never moved into a local here.
        let mut record = record.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        Some(act(record.as_mut()?))
    }
}

#[cfg(unix)]
impl Drop for PrivateOrderedContinuationSlot {
    /// A slot that was neither installed into nor finished.
    ///
    /// Not released, and not silently forgotten. Releasing would hand the
    /// capacity out again while whatever this connection accepted is
    /// unaccounted for; forgetting would make that invisible. It is marked, so
    /// the place stays taken and the fact that nobody disposed of it can be
    /// read.
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut held = self.owner.records_even_if_poisoned();
        held.continuations_abandoned = held.continuations_abandoned.saturating_add(1);
    }
}
