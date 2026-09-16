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
/// UNREADABLE IS NOT FENCED. A gate whose lock is poisoned cannot be shown to
/// have stopped anything, and reporting one as the other would name a fence
/// that was never established -- a caller would then go on to end a socket
/// under handovers that may still be running.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Teardown drives closing; not attached yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrivateHandoverFence {
    /// This close made the closure, and no handover was in progress after it.
    Established,
    /// The closure was already made. Still a fence, and still exact.
    AlreadyEstablished,
    /// Nothing could be established. Retained failure, not a fence.
    Unreadable,
}

/// Why a handover was not admitted.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrivateHandoverRefusal {
    /// This registration's endpoint is closed. Nothing more may be handed to
    /// it, and this producer took nothing.
    Fenced,
    /// The gate could not be read, so admission could not be established.
    /// Refused for the same reason a fence would be: an unestablished answer
    /// is not permission.
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

    /// Whether this registration's closure has been made.
    ///
    /// `None` for a gate that cannot be read, which is neither answer.
    #[cfg_attr(not(test), allow(dead_code))] // Only controls ask this today.
    fn fenced(&self) -> Option<bool> {
        self.fenced.lock().ok().map(|fenced| *fenced)
    }
}

/// A recipient's ordered queue, reachable only through its gate.
///
/// The sender is private on purpose. A producer that could clone the channel
/// out of this would have the capture back without the serialization, which is
/// the whole defect.
#[cfg(unix)]
#[derive(Clone)]
pub(crate) struct PrivateGatedOrderedSender {
    sender: SyncSender<XAuthorityOrderedDelivery>,
    gate: Arc<PrivateHandoverGate>,
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
            sender: &self.sender,
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
