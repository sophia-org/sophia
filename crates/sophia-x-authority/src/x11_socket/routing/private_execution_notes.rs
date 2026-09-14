// What one guarded transition reports on its way out.
//
// Split by subject from the transition itself: that file is about entering
// the ledger and the source in the right order, this is about the vocabulary
// the transition answers in. The two change for different reasons -- a new
// fact to record is not a new step to take -- and the transition file is the
// one that has to stay readable end to end.

/// What the guarded transition recorded on its way out.
///
/// Out-parameters rather than a return value: the transaction's result is the
/// authority's, and these are facts about what happened inside it that the
/// authority has no vocabulary for. Collected in one place so that recording
/// another fact does not mean threading another argument.
#[cfg(unix)]
struct PrivateTransactionNotes<'a> {
    /// What was decided, if anything was.
    decided: Option<PrivateOrderedDecision>,
    /// A hold ended and the record of where its press went is gone.
    plan_missing: bool,
    /// This executor already holds as many records as it may.
    records_exhausted: bool,
    /// The ledger will not carry this delivery to its recipient.
    delivery_ended: bool,
    /// The ledger could not be read.
    recovery_unavailable: bool,
    /// What the native source refused, when it refused.
    ///
    /// Carried out rather than renamed. The source tells a delivery that ended
    /// from a ledger nobody could read from a selection that was not there
    /// from an origin that was not ours, and an authority error standing in
    /// for all four would lose which one happened.
    native_refusal: Option<private_native::Refusal>,
    /// The execution a supervisor is watching.
    ///
    /// Borrowed from whoever owns the watch and finishes it, and carried with
    /// the notes rather than as another argument because the phases it records
    /// belong beside the marker they describe: the two points that say an
    /// effect may have happened are the two a supervisor has to hear about.
    watched: &'a mut private_watchdog::PrivateWatchedExecution,
    /// Whether an effect may have reached the authority's ledger.
    ///
    /// Set before each call that can move it, never after: a marker written
    /// afterwards says nothing about a call that did not return. Shared with
    /// the claim guard rather than copied to it, so an error returned out of
    /// the transaction carries the same answer an unwind does.
    may_have_applied: &'a std::cell::Cell<bool>,
}

#[cfg(unix)]
impl<'a> PrivateTransactionNotes<'a> {
    fn new(
        may_have_applied: &'a std::cell::Cell<bool>,
        watched: &'a mut private_watchdog::PrivateWatchedExecution,
    ) -> Self {
        Self {
            watched,
            decided: None,
            plan_missing: false,
            records_exhausted: false,
            delivery_ended: false,
            recovery_unavailable: false,
            native_refusal: None,
            may_have_applied,
        }
    }
}

/// An execution's hold on a delivery, given back however the execution ends.
///
/// A guard, because giving it back is the part that must not be skipped. An
/// unwind between the claim and the end of the transaction would otherwise
/// leave a delivery nothing can cancel again, and it resolves as
/// possibly-applied: the direction that cannot publish a cancellation over an
/// effect that happened.
#[cfg(unix)]
struct PrivateDeliveryClaim<'a> {
    recovery: &'a InputRecovery,
    delivery: Option<XAuthorityInputDeliveryId>,
    /// Read at drop, not at construction. The transaction writes through this
    /// as it goes, so every way out of the execution -- a decision, an error
    /// returned from a fallible call, an unwind -- gives the claim back with
    /// what had actually happened by then.
    applied: &'a std::cell::Cell<bool>,
}

#[cfg(unix)]
impl Drop for PrivateDeliveryClaim<'_> {
    fn drop(&mut self) {
        self.recovery.resolve_claim(self.delivery, self.applied.get());
    }
}

/// What the guarded transition decided, before anything is emitted.
#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
struct PrivateOrderedDecision {
    owes_event: bool,
    reached: Option<PrivateReachedResources>,
    first_press: bool,
    keyboard_applied: bool,
    release: Option<sophia_input_authority::ReleaseOutcome>,
    event: Option<XAuthorityInputEvent>,
}
