// How a delivery's answer is owned, and who is allowed to give it.
//
// Split by subject from the ledger beside it: that file is about what recovery
// knows and decides about deliveries, this is about the one handle a writer
// answers through and the cell that handle writes into. They change for
// different reasons -- a new thing to know about a delivery is not a new way
// to answer one.

/// What became of one offered answer, said by the branch that decided it.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivateTerminalDisposition {
    /// This answer was published.
    Recorded,
    /// THIS answer was held under a claim, to be decided when it resolves.
    Deferred,
    /// This answer was declined. Something else may be held for the same
    /// delivery; that is not this offer's fate.
    Rejected,
}

/// What the terminal authority did with an answer it was offered.
///
/// A BOOLEAN COULD NOT SAY THIS. Returning true whenever the authority was
/// called reported success for an answer it had silently rejected; returning
/// false for a pruned admission stranded a writer whose own cell already held
/// the answer. Both are states the authority owns, and they are not refusals.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivateAdjudication {
    /// The authority recorded this answer.
    Answered,
    /// This admission already had its answer. Nothing more is owed for it.
    AlreadyAnswered,
    /// The authority is holding it under a claim, to decide when that
    /// resolves. It has been taken, and is no longer the writer's to answer.
    Deferred,
    /// Nothing was adjudicated: the ledger could not be read, the admission is
    /// gone with no answer, the entry is not the one this finalizer was made
    /// for, or the authority declined it. The caller still owes an answer.
    Refused,
}

/// Build a finalizer from a completion its holder already has.
///
/// NO ACQUISITION. The handle comes from the debt that has carried it since
/// the operation that created it; looking one up by delivery id here is the
/// late raw-id acquisition that a prune and a re-admission defeat, and it is
/// what this constructor exists to avoid.
#[cfg(unix)]
fn finalizer_from_held(
    recovery: &InputRecovery,
    completion: &Arc<PrivateDeliveryCompletion>,
    delivery: XAuthorityInputDeliveryId,
    client: XServerFrontendClientId,
) -> PrivateDeliveryFinalizer {
    PrivateDeliveryFinalizer {
        recovery: recovery.clone(),
        completion: Arc::clone(completion),
        delivery: Some(delivery),
        client,
        unadmitted: None,
    }
}

/// Build a finalizer for a delivery nobody admitted.
///
/// A release the ledger made when its source departed has no request behind
/// it, so recovery never admitted it: there is no delivery id, no ticket and
/// no compositor receipt to publish. What its writer can still say is what
/// became of the bytes, and that goes into the cell its custody holds. The
/// completion carried here is a fresh, never-answered cell, present because
/// every finalizer names one; the answer lives in `unadmitted`.
#[cfg(unix)]
fn finalizer_for_unadmitted(
    recovery: &InputRecovery,
    unadmitted: &Arc<PrivateUnadmittedCompletion>,
    client: XServerFrontendClientId,
) -> PrivateDeliveryFinalizer {
    PrivateDeliveryFinalizer {
        recovery: recovery.clone(),
        completion: Arc::default(),
        delivery: None,
        client,
        unadmitted: Some(Arc::clone(unadmitted)),
    }
}

/// The one way a writer answers the delivery it was given.
///
/// ORIGIN-BOUND. Writing into the completion cell directly recorded an answer
/// that the ledger itself never saw: its ticket stayed unanswered, no ordinary
/// observer was told, and a later disconnect could set a different terminal
/// outcome while the cell still said the first one. Two accounts of one
/// delivery, disagreeing, with nothing to adjudicate between them.
///
/// Everything goes through the single terminal authority now -- the cell, the
/// ticket, the claim arbitration and the notification are all decided in one
/// place, so every account agrees on the outcome that was adjudicated.
///
/// IDENTITY IS THE CARRIED CELL. The delivery id is used to find the entry and
/// the entry's own completion is then compared against the one this finalizer
/// holds; a number that has been pruned and handed out again does not match,
/// so a writer can never answer an admission other than its own.
#[cfg(unix)]
pub(crate) struct PrivateDeliveryFinalizer {
    recovery: InputRecovery,
    completion: Arc<PrivateDeliveryCompletion>,
    /// `None` for a delivery nobody admitted: it has no identity to name.
    delivery: Option<XAuthorityInputDeliveryId>,
    client: XServerFrontendClientId,
    /// Where the answer goes when nobody admitted this delivery. Exclusive
    /// with `delivery` being `Some`.
    unadmitted: Option<Arc<PrivateUnadmittedCompletion>>,
}

#[cfg(unix)]
impl std::fmt::Debug for PrivateDeliveryFinalizer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrivateDeliveryFinalizer")
            .field("delivery", &self.delivery)
            .field("client", &self.client)
            .field("unadmitted", &self.unadmitted.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(unix)]
impl PrivateDeliveryFinalizer {
    /// Answer this delivery, through the authority that owns the answer.
    ///
    /// Returns what the authority did with this offer. Only Refused leaves the
    /// answer still owed -- the ledger could not be read, the admission is
    /// gone with no answer, the entry is not the one this finalizer was made
    /// for, or the offer was declined.
    pub(crate) fn finalize(&self, outcome: XAuthorityInputDeliveryOutcome) -> PrivateAdjudication {
        match (&self.unadmitted, self.delivery) {
            // ANSWERED INTO THE CUSTODY'S OWN CELL, and never refused: a refusal
            // would leave the writer owing an answer for ever, since no
            // authority will ever come asking for this one. Written once; a
            // second outcome for one write is a contradiction and the first
            // stands.
            (Some(cell), _) => {
                if cell.publish(outcome) {
                    PrivateAdjudication::Answered
                } else {
                    PrivateAdjudication::AlreadyAnswered
                }
            }
            (None, Some(delivery)) => {
                self.recovery
                    .adjudicate_for_held(&self.completion, self.client, delivery, outcome)
            }
            (None, None) => PrivateAdjudication::Refused,
        }
    }
}

/// Where a writer answers a delivery nobody admitted.
///
/// OUTCOME ONLY. It names no delivery and no client, because there is no
/// admission to name and no compositor receipt to publish; what it holds is
/// the one thing the writer can establish about bytes it was handed, and the
/// custody that owns this cell is the only reader. Written once, like the
/// admitted completion beside it.
#[cfg(unix)]
#[derive(Debug, Default)]
pub(crate) struct PrivateUnadmittedCompletion {
    outcome: std::sync::OnceLock<XAuthorityInputDeliveryOutcome>,
}

#[cfg(unix)]
impl PrivateUnadmittedCompletion {
    /// Record the writer's answer; false if one already stands.
    pub(crate) fn publish(&self, outcome: XAuthorityInputDeliveryOutcome) -> bool {
        self.outcome.set(outcome).is_ok()
    }

    pub(crate) fn answer(&self) -> Option<XAuthorityInputDeliveryOutcome> {
        self.outcome.get().copied()
    }
}

/// The one place a delivery's terminal outcome is ever written.
///
/// MINTED BY THIS LEDGER AT ADMISSION, before anything can be accepted for the
/// delivery it belongs to. A holder of this cell holds the completion of that
/// exact admission and of no other: a delivery id that is pruned and admitted
/// again gets a NEW cell, so an old holder can never see the new admission's
/// answer however the number is reused.
///
/// Shared rather than copied out. The ordinary observer prunes the ticket as
/// soon as it consumes the outcome, and a reader that had to go back to the
/// ticket for its answer would find the answer gone. Anyone who took custody
/// of this cell keeps the answer whether or not the ticket still exists.
///
/// Written once. A second answer for one admission is a contradiction rather
/// than an update, and the first one stands.
#[cfg(unix)]
#[derive(Debug, Default)]
pub(crate) struct PrivateDeliveryCompletion {
    outcome: std::sync::OnceLock<XAuthorityClientInputDelivery>,
}

#[cfg(unix)]
impl PrivateDeliveryCompletion {
    pub(crate) fn publish(&self, receipt: XAuthorityClientInputDelivery) {
        let _ = self.outcome.set(receipt);
    }

    pub(crate) fn answer(&self) -> Option<XAuthorityClientInputDelivery> {
        self.outcome.get().copied()
    }
}
