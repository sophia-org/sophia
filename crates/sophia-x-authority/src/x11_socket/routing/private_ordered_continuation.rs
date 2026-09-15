// What one connection's ordered output still owes after that connection ends.
//
// Split by subject from the settlement owner that stores it. That owner
// answers what happens to work an instance abandoned; this answers what one
// connection's ordered output IS when it is handed over, and what reserving a
// place for it costs before the connection is allowed to exist.

/// A connection's ordered output, retained whole.
///
/// NOT UNPACKED, and not turned back into anything replayable. What is here
/// has applied, or may already be on a recipient's queue, so the only thing
/// carried is the right to go on answering for it.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // The dispatch binding is not landed yet.
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
    },
    /// A serving owner, whole: its endpoint, receiver, queued contents,
    /// in-flight frame and offset, received-but-unclassified and foreign
    /// capsules, unanswered records with their own finalizers, the shared
    /// output, permission and stop handles, its independent shutdown handle,
    /// and whatever its close has established so far.
    Serving(Box<X11OrderedServingOwner>),
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
#[cfg_attr(not(test), allow(dead_code))] // The dispatch binding is not landed yet.
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
    /// Promised to a connection that has not handed anything over yet.
    Reserved,
    /// Holding one connection's continuation.
    ///
    /// SEPARATELY OWNED, deliberately. Driving a continuation means calling
    /// into its close, which takes this connection's output and its
    /// finalizers; doing that under the aggregate lock would put the whole
    /// store behind one connection's write, and would take common beneath
    /// settlement. The record has its own lock, so the aggregate one is held
    /// only long enough to find it.
    Held(Arc<Mutex<PrivateOrderedContinuation>>),
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
#[cfg_attr(not(test), allow(dead_code))] // The dispatch binding is not landed yet.
struct PrivateOrderedContinuationSlot {
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

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // The dispatch binding is not landed yet.
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
    fn install(mut self, source: &mut Option<PrivateOrderedContinuation>) {
        let mut held = self.owner.records_even_if_poisoned();
        debug_assert!(
            matches!(
                held.continuations[self.index],
                PrivateOrderedContinuationPlace::Reserved
            ),
            "this place is this slot's and holds nothing yet"
        );
        let Some(continuation) = source.take() else {
            // Nothing to install. The place stays this connection's, because
            // the reservation is not returned by an install that had nothing.
            drop(held);
            self.armed = false;
            return;
        };
        held.continuations[self.index] =
            PrivateOrderedContinuationPlace::Held(Arc::new(Mutex::new(continuation)));
        self.armed = false;
    }

    /// Give the place back, for a connection that finished owing nothing.
    ///
    /// Only for that. A slot returned while its owner still holds unanswered
    /// admissions, foreign capsules or a wire whose ending was never
    /// established would be capacity handed out against work that still
    /// exists.
    fn finish(mut self) {
        let mut held = self.owner.records_even_if_poisoned();
        debug_assert!(
            matches!(
                held.continuations[self.index],
                PrivateOrderedContinuationPlace::Reserved
            ),
            "a finished slot holds nothing"
        );
        held.continuation_slots = held.continuation_slots.saturating_sub(1);
        held.continuations[self.index] = PrivateOrderedContinuationPlace::Free;
        self.armed = false;
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // The dispatch binding is not landed yet.
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
            Self::Serving(owner) => &owner.queue,
        }
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // The dispatch binding is not landed yet.
impl PrivateSettlementOwner {
    /// Borrow one retained continuation, if the place holds one.
    ///
    /// BORROWED, NOT TAKEN OUT. Driving it means calling into the close, which
    /// takes this connection's output and its finalizers; doing that with the
    /// record in a local would put a retained continuation in a stack frame
    /// across exactly the calls that can unwind.
    ///
    /// The aggregate lock is NOT held across that work. Its order here is
    /// settlement before anything the close touches, and holding it while a
    /// close waits on output serialization would invert that.
    fn with_ordered_continuation<R>(
        &self,
        index: usize,
        act: impl FnOnce(&mut PrivateOrderedContinuation) -> R,
    ) -> Option<R> {
        // The aggregate lock is held only to find the record's own handle.
        let record = {
            let held = self.records_even_if_poisoned();
            let PrivateOrderedContinuationPlace::Held(record) = held.continuations.get(index)?
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
        Some(act(&mut record))
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
