// How a delivery's answer is owned, and who is allowed to give it.
//
// Split by subject from the ledger beside it: that file is about what recovery
// knows and decides about deliveries, this is about the one handle a writer
// answers through and the cell that handle writes into. They change for
// different reasons -- a new thing to know about a delivery is not a new way
// to answer one.

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
        delivery,
        client,
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
    delivery: XAuthorityInputDeliveryId,
    client: XServerFrontendClientId,
}

#[cfg(unix)]
impl std::fmt::Debug for PrivateDeliveryFinalizer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrivateDeliveryFinalizer")
            .field("delivery", &self.delivery)
            .field("client", &self.client)
            .finish_non_exhaustive()
    }
}

#[cfg(unix)]
impl PrivateDeliveryFinalizer {
    /// Answer this delivery, through the authority that owns the answer.
    ///
    /// Returns whether that authority took it. False means nothing was
    /// adjudicated -- the ledger could not be read, the admission is gone, or
    /// the entry is no longer the one this finalizer was made for -- and the
    /// caller still owes this delivery an answer.
    pub(crate) fn finalize(&self, outcome: XAuthorityInputDeliveryOutcome) -> PrivateAdjudication {
        self.recovery
            .adjudicate_for_held(&self.completion, self.client, self.delivery, outcome)
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
