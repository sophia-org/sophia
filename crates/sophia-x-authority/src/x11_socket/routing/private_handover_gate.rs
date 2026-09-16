/// Where a registration's handovers and its closing are put in one order.
///
/// THE ROW IS NOT THE FENCE. A producer finds a recipient by taking the client
/// table, cloning the sender out of the row and releasing the table before it
/// hands anything over -- it has to, because taking custody beneath the client
/// table would hold it across work that must not run there. So a sender can be
/// captured before a close and used after it, and removing the row or reading
/// a flag at lookup time cannot see that: by then the capture already happened.
///
/// The gate is what the two share. A producer acquires it before it takes
/// custody of anything and holds it through the handover and the recording of
/// what came back; a close acquires the same gate and makes closure
/// irreversible. Either the producer was already inside, in which case the
/// close waits and the handover completes, or it was not, in which case it is
/// refused and takes nothing. There is no third outcome to account for, and no
/// counter of work in flight to keep right.
///
/// BOUND TO ONE REGISTRATION, minted with that registration's queue and before
/// its row is published. A replacement registration for the same client mints
/// its own, so closing an old endpoint cannot reach a newer one's handovers.
#[cfg(unix)]
pub(crate) struct PrivateHandoverGate {
    fenced: Mutex<bool>,
}

/// What a close established.
///
/// UNREADABLE IS NOT FENCED -- but not because exclusion failed. A poisoned
/// lock is acquired and then reported as poisoned; the guard comes back inside
/// the error, so nothing was running beside a close that saw one. What a
/// poisoned gate says is that a holder PANICKED while inside it, which leaves
/// two things unestablished: the flag under it may not reflect a completed
/// operation, and the handover that panicked may have left custody
/// unresolved. Reporting that as a fence would name something established over
/// a connection that still has an unanswered question about it.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Teardown drives closing; not attached yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrivateHandoverFence {
    /// This close made the closure, and no handover was in progress after it.
    Established,
    /// The closure was already made. Still a fence, and still exact.
    AlreadyEstablished,
    /// A holder panicked inside this gate. The lock was still acquired -- that
    /// is what poisoning means -- so this is not a failure to exclude; it is a
    /// closure that cannot be trusted to have been made over resolved custody.
    /// Retained failure, not a fence.
    Unreadable,
}

/// Why a handover was not admitted.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrivateHandoverRefusal {
    /// This registration's endpoint is closed. Nothing more may be handed to
    /// it, and this producer took nothing.
    Fenced,
    /// A holder panicked inside this gate. This producer did acquire it -- a
    /// poisoned lock is an acquired lock -- and is refused anyway: the flag it
    /// would be trusting may not reflect a completed operation, and an
    /// unestablished answer is not permission. Refusing takes nothing, so the
    /// cost of being wrong here is a retry rather than a lost capsule.
    Unreadable,
}

#[cfg(unix)]
impl PrivateHandoverGate {
    fn open() -> Self {
        Self {
            fenced: Mutex::new(false),
        }
    }

    /// Make this registration's closure, irreversibly.
    ///
    /// SERIALIZED WITH HANDOVERS, not with lookups. A handover already inside
    /// finishes first and this waits for it; one that has only captured a
    /// sender finds the closure made and is refused.
    ///
    /// Released before the caller goes on. Ending a socket, answering
    /// finalizers and settling anything are separate facts, and doing them
    /// under this would put them beneath a lock every producer takes.
    #[cfg_attr(not(test), allow(dead_code))] // Teardown drives closing; not attached yet.
    fn close(&self) -> PrivateHandoverFence {
        let Ok(mut fenced) = self.fenced.lock() else {
            return PrivateHandoverFence::Unreadable;
        };
        if *fenced {
            return PrivateHandoverFence::AlreadyEstablished;
        }
        *fenced = true;
        PrivateHandoverFence::Established
    }

    /// Hold this gate for something that is not a handover.
    ///
    /// SAME SERIALIZATION, DIFFERENT PURPOSE. A close and a handover are put
    /// in one order by this gate; so is anything else that must not straddle a
    /// close. The guard is what makes a check and the act that depends on it
    /// one thing rather than two with a window between them.
    ///
    /// Refuses a closed endpoint and an unreadable gate differently, because
    /// they are different: one says this endpoint is done, the other says a
    /// holder panicked inside and what the flag says cannot be trusted.
    fn entered(&self) -> Result<std::sync::MutexGuard<'_, bool>, PrivateHandoverRefusal> {
        let Ok(fenced) = self.fenced.lock() else {
            return Err(PrivateHandoverRefusal::Unreadable);
        };
        if *fenced {
            return Err(PrivateHandoverRefusal::Fenced);
        }
        Ok(fenced)
    }

    /// Whether this registration's closure has been made.
    ///
    /// `None` for a gate that cannot be read, which is neither answer.
    #[cfg_attr(not(test), allow(dead_code))] // Only controls ask this today.
    fn fenced(&self) -> Option<bool> {
        self.fenced.lock().ok().map(|fenced| *fenced)
    }
}

/// What a connection's waiter is told, and by whom.
///
/// LEVELS, NOT EDGES. Each of these stays set until whoever acts on it clears
/// it, so a notice cannot fall between a waiter deciding and a waiter
/// sleeping: the waiter holds this lock across that decision, and the sleep is
/// the atomic release.
///
/// NONE OF THEM IS EVIDENCE. `gone` says the senders are finished with; only
/// the owner's own receive returning Disconnected establishes that, and only
/// that may be recorded. A notice is a reason to look.
#[cfg(unix)]
#[derive(Debug)]
struct PrivateWakeState {
    /// Something may have been accepted since the last look.
    ///
    /// Set by the handover notification, which is not landed: only the
    /// disappearance below publishes anything today.
    #[cfg_attr(not(test), allow(dead_code))]
    pending: bool,
    /// How many senders for this connection exist.
    ///
    /// COUNTED BY THE WRAPPER THAT MAKES THEM. Not an Arc strong count: that
    /// answers how many references to a shared thing exist, which is a
    /// different question with no owner, and it is nobody's decision point.
    /// This moves in exactly two places -- a wrapper being cloned and a
    /// wrapper being dropped -- so the step to zero happens once, where it can
    /// be acted on.
    senders: usize,
    /// Every sender is gone. A HINT TO LOOK AGAIN, not a finding.
    gone: bool,
}

/// One connection's waitable notice.
///
/// Minted with its queue and held by both halves, so there is exactly one per
/// connection and the declared client limit already bounds how many exist.
/// Carries no payload: what is waiting is on the queue, and only the queue's
/// owner may take it.
#[cfg(unix)]
struct PrivateOrderedWake {
    state: Mutex<PrivateWakeState>,
    ready: std::sync::Condvar,
}

#[cfg(unix)]
impl PrivateOrderedWake {
    /// A notice for a connection whose first sender is being made.
    fn for_first_sender() -> Self {
        Self {
            state: Mutex::new(PrivateWakeState {
                pending: false,
                senders: 1,
                gone: false,
            }),
            ready: std::sync::Condvar::new(),
        }
    }

    /// Say that the senders are finished with, and wake whoever is waiting.
    ///
    /// CALLED ONLY AFTER THE LAST SENDER IS ACTUALLY GONE. A notice published
    /// while a sender still exists wakes a waiter that receives, finds the
    /// queue merely empty, and goes back to sleep -- and the drop that follows
    /// wakes nobody, which is the case this whole mechanism exists for.
    ///
    /// DOES NOT PANIC. A poisoned notice is recovered rather than unwrapped,
    /// because this runs in a Drop that may be running during an unwind, where
    /// a panic would abort. The signal happens whether or not the flag could
    /// be set, and outside the lock, because a waiter that cannot be told the
    /// reason must at least be made to look.
    fn publish_disappearance(&self) {
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.gone = true;
        }
        self.ready.notify_all();
    }
}

/// A recipient's ordered queue, reached through its gate.
///
/// The sender field is private, but that is a smaller guarantee than it looks:
/// the routing sources are textually included into one module, so anything in
/// that module could reach `sender` directly and send around the gate. This
/// keeps an accidental clone from compiling elsewhere; it does not make a
/// bypass impossible, and no type here can. What is established is narrower
/// and has to be re-established as producers are added: the two production
/// ordered sends that exist both go through `admit`.
#[cfg(unix)]
pub(crate) struct PrivateGatedOrderedSender {
    /// `None` only while this wrapper is being dropped.
    ///
    /// Held in an Option so the drop order can be chosen rather than
    /// inherited: a field destroyed automatically is destroyed AFTER the Drop
    /// body, which is exactly the wrong side of the notification.
    sender: Option<SyncSender<XAuthorityOrderedDelivery>>,
    gate: Arc<PrivateHandoverGate>,
    wake: Arc<PrivateOrderedWake>,
}

/// COUNTED WHERE SENDERS ARE MADE. Cloning this is the only way another sender
/// for a connection comes to exist, so the count moves here and nowhere else.
///
/// THAT IS A DISCIPLINE, NOT A BARRIER. These routing sources are textually
/// included into one module, so anything in it could reach the raw sender and
/// clone it behind this count. What holds today is checkable and was checked:
/// the field is touched in exactly four places, all of them here -- this
/// clone, the drop, the admission, and the send through it -- and nothing
/// outside this file names it. A new caller that reaches past it would have to
/// be added on purpose, and would make the count wrong silently, so this is
/// re-established by reading rather than guaranteed by the type.
#[cfg(unix)]
impl Clone for PrivateGatedOrderedSender {
    fn clone(&self) -> Self {
        {
            let mut state = self
                .wake
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.senders = state.senders.saturating_add(1);
        }
        Self {
            sender: self.sender.clone(),
            gate: self.gate.clone(),
            wake: self.wake.clone(),
        }
    }
}

#[cfg(unix)]
impl Drop for PrivateGatedOrderedSender {
    /// THE SENDER GOES FIRST, AND THEN THE NOTICE.
    ///
    /// A Drop body runs before this struct's fields are destroyed, so
    /// decrementing and signalling here and letting the sender field fall away
    /// afterwards publishes "they are all gone" while this one still exists. A
    /// waiter woken then receives, finds the queue empty rather than finished,
    /// and sleeps again -- and the drop that really ends it signals nobody.
    ///
    /// So the sender is taken out and dropped explicitly, and only then is the
    /// count moved and the notice published.
    fn drop(&mut self) {
        drop(self.sender.take());
        let last = {
            let mut state = self
                .wake
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.senders = state.senders.saturating_sub(1);
            state.senders == 0
        };
        if last {
            self.wake.publish_disappearance();
        }
    }
}

#[cfg(unix)]
impl PrivateGatedOrderedSender {
    /// Ask to hand over, BEFORE taking custody of anything.
    ///
    /// The admission is held for the handover and for writing down what came
    /// back, so a close cannot land between the send and the record of it.
    fn admit(&self) -> Result<PrivateHandoverAdmission<'_>, PrivateHandoverRefusal> {
        let Ok(fenced) = self.gate.fenced.lock() else {
            return Err(PrivateHandoverRefusal::Unreadable);
        };
        if *fenced {
            return Err(PrivateHandoverRefusal::Fenced);
        }
        Ok(PrivateHandoverAdmission {
            sender: self
                .sender
                .as_ref()
                .expect("a sender is present until its wrapper is dropped"),
            _held: fenced,
        })
    }
}

/// Permission to hand one delivery over, held for as long as it takes.
///
/// Dropped before any give-back, finalizer or common-side work: those take
/// locks of their own, and taking them beneath this would put every producer
/// behind them.
#[cfg(unix)]
pub(crate) struct PrivateHandoverAdmission<'a> {
    sender: &'a SyncSender<XAuthorityOrderedDelivery>,
    _held: std::sync::MutexGuard<'a, bool>,
}

#[cfg(unix)]
impl PrivateHandoverAdmission<'_> {
    /// Hand the delivery over, or hand it straight back.
    ///
    /// The capsule comes back exactly as it went in, which is what lets a
    /// refused handover be retried with the bytes and the identity the
    /// release already decided.
    // THE CAPSULE COMES BACK IN THE ERROR, which is the point: a refused
    // handover is retried with the bytes and the identity the release already
    // decided, not with a rebuilt copy. Boxing it to make the error small
    // would allocate on the refusal path and buy nothing.
    #[allow(clippy::result_large_err)]
    fn try_send(
        &self,
        capsule: XAuthorityOrderedDelivery,
    ) -> Result<(), std::sync::mpsc::TrySendError<XAuthorityOrderedDelivery>> {
        self.sender.try_send(capsule)
    }
}
