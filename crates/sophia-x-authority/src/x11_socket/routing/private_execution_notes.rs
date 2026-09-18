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
    /// The terminal owner installs a source witness before common defers.
    freeze: Option<&'a mut Option<private_native::Freeze>>,
    deferred: bool,
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
    /// This delivery has no completion to answer it with.
    completion_missing: bool,
    /// A previous operation's custody is still held with no disposition.
    custody_retained: bool,
    /// No unique order stamp was available for this event.
    order_exhausted: bool,
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
            freeze: None,
            deferred: false,
            watched,
            decided: None,
            plan_missing: false,
            records_exhausted: false,
            delivery_ended: false,
            recovery_unavailable: false,
            completion_missing: false,
            custody_retained: false,
            order_exhausted: false,
            native_refusal: None,
            may_have_applied,
        }
    }

    fn freeze_witness(&self) -> Option<&private_native::Freeze> {
        self.freeze.as_ref().and_then(|slot| slot.as_ref())
    }

    fn defer_freeze(
        &mut self,
        checked: Result<private_native::FreezeCheck, private_native::Refusal>,
    ) -> Result<bool, sophia_input_authority::RegistrationError> {
        match checked {
            Ok(private_native::FreezeCheck::Ready) => Ok(false),
            Ok(private_native::FreezeCheck::Frozen(witness)) => {
                if let Some(slot) = self.freeze.as_mut() {
                    **slot = Some(witness);
                    self.deferred = true;
                    Ok(true)
                } else {
                    self.native_refusal = Some(private_native::Refusal::KeyboardFrozen);
                    Err(sophia_input_authority::RegistrationError::RoutingUnavailable)
                }
            }
            Err(cause) => {
                self.native_refusal = Some(cause);
                Err(sophia_input_authority::RegistrationError::RoutingUnavailable)
            }
        }
    }
}

#[cfg(unix)]
#[expect(clippy::large_enum_variant, reason = "The completed decision moves directly into reserved terminal storage without a per-attempt allocation.")]
enum PrivateExecutionAttempt {
    Completed(PrivateOrderedRun),
    Deferred,
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
    completion: Option<&'a Arc<PrivateDeliveryCompletion>>,
    /// Read at drop, not at construction. The transaction writes through this
    /// as it goes, so every way out of the execution -- a decision, an error
    /// returned from a fallible call, an unwind -- gives the claim back with
    /// what had actually happened by then.
    applied: &'a std::cell::Cell<bool>,
}

#[cfg(unix)]
impl Drop for PrivateDeliveryClaim<'_> {
    fn drop(&mut self) {
        match (self.delivery, self.completion) {
            (Some(delivery), Some(cell)) => {
                self.recovery.resolve_claim_for_held(delivery, cell, self.applied.get());
            }
            _ => self.recovery.resolve_claim(self.delivery, self.applied.get()),
        }
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

// What one guarded transition can refuse with, and what it reports when it
// does not. Moved here from the transition itself: this file is the
// vocabulary a transition answers in, and a refusal is part of that answer.

/// Why an ordered execution did not apply.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivateExecutionRefusal {
    /// The original recovery cell is missing or has been replaced. This is
    /// not proof that the accepted original delivery ended.
    CompletionMismatch(PrivateCompletionMismatch),
    /// The keyboard state offered is not this instance's.
    ForeignKeyboards,
    /// This seat has no keyboard state and one could not be built. Refused
    /// before the transaction, where refusing is still free.
    SeatUnavailable,
    /// The input does not name anything this authority can validate.
    Unmappable,
    /// StateOnly supports key releases without a delivery cell. Other shapes
    /// cannot be treated as an ordinary delivery or a physical key release.
    StateOnlyUnsupported,
    /// Repetition is a delivery policy, not another aggregate press or join.
    /// The private source does not yet own an ordered repeat operation.
    RepeatUnsupported,
    /// This executor already holds as many records as it may.
    ///
    /// Refused before the effect, so nothing is applied that could not then be
    /// recorded -- a hold whose plan has nowhere to go is a release nobody can
    /// answer.
    RecordsExhausted,
    /// The ledger owes this release a delivery and the plan recording where
    /// its press went is not here.
    ///
    /// Not the same as owing nobody an event. A hold that ended has a
    /// recipient by definition, so an absent record is an obligation nobody
    /// can currently discharge -- reporting it as nothing to emit would settle
    /// a debt by losing the evidence of it.
    HoldPlanMissing,
    /// No supervisor is watching this execution.
    ///
    /// Refused rather than run unwatched. The watch exists for the case where
    /// an execution does not come back, and starting one that nothing is
    /// watching is starting the case it was meant to catch with nothing left
    /// to catch it.
    Unwatched,
    /// This instance has no prepared native origin.
    ///
    /// Refused rather than pressed without one. Every hold clones that origin
    /// and a proof is checked against it, so a press that began without one
    /// would leave a hold nothing could ever prove anything about.
    NativeUnprepared,
    /// Another execution holds this delivery.
    ///
    /// Its effect may be under way, so this one may not apply a second. Not
    /// the same as ended: nothing has finished, and the delivery is still owed
    /// an outcome by whoever holds it.
    DeliveryClaimedElsewhere,
    /// The ledger will not carry this delivery to a recipient.
    ///
    /// Either a terminal outcome was already recorded for it -- revoked with
    /// its epoch, timed out, or disconnected with its client while it waited
    /// its turn -- or binding it to the recipient found that connection
    /// already revoked and recorded one now. Both are decisions, and in both
    /// the work must not be applied: an effect for a delivery whose outcome
    /// is already reported would be an effect nobody is waiting for.
    DeliveryEnded,
    /// No unique order could be assigned to this event.
    ///
    /// The stamp says where an event sits in the order its recipient must see,
    /// and a reused one would put two events in the same place. Refused before
    /// the effect rather than saturating: an order that repeats is not an
    /// order.
    OrderExhausted,
    /// A previous operation's custody is still held here and has had no
    /// disposition.
    ///
    /// Refused rather than replaced. A refusal that left the source holding
    /// context leaves this custody attached to that same continuation, and
    /// overwriting it would drop the only handle able to answer whatever that
    /// continuation still owes -- silently, with nothing recorded about what
    /// became of it.
    CustodyRetained,
    /// This delivery has no completion, so its answer could never be matched
    /// to the debt it belongs to.
    ///
    /// A KNOWN ABSENCE, told apart from an unreadable ledger: this one says
    /// the delivery will never be answerable, the other says nothing was
    /// established either way. Both refuse before the effect, and reporting
    /// them under one cause discarded the distinction where it mattered.
    CompletionMissing,
    /// The delivery ledger could not be read.
    ///
    /// Not the same as ended. Nothing is known about whether this delivery is
    /// still owed an outcome, and executing on that would create a hold this
    /// executor cannot prove anyone is waiting for.
    RecoveryUnavailable,
    /// The item was taken from the order and execution had not been attempted.
    ///
    /// The phase a current item carries while it is owned and before its
    /// execution returns, so an interruption leaves a record that says what
    /// was and was not tried.
    NotAttempted,
    /// The transaction returned without deciding.
    ///
    /// Carries the completion the authority actually recorded, because that is
    /// the cause. Discarding it and naming a plausible error here would
    /// replace what happened with a guess about it.
    NotDecided(sophia_input_authority::RequestCompletion),
    /// The source refused, under the name the source gave it.
    ///
    /// The source distinguishes a delivery that ended, a ledger nobody could
    /// read, a selection that was not there and an origin that was not ours.
    /// All of them leave the transaction carrying one authority error, so
    /// reporting that error would say only that something went wrong inside.
    /// Recording the cause and never reading it would be worse still: a fact
    /// written down where nothing can reach it is not a fact anyone has.
    Native(private_native::Refusal),
    /// The admission boundary refused.
    Admission(PrivateAdmissionRefusal),
    /// The authority refused.
    Authority(PrivateAuthorityRefusal),
}

/// What one ordered input did.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivateOrderedRun {
    /// Where it went, as decided under the guards.
    ///
    /// `None` where nothing was owed a delivery. That is an outcome, not a
    /// failure to find a target.
    pub reached: Option<PrivateReachedResources>,
    /// Whether this press began the hold rather than joining one.
    ///
    /// A join moves the ledger without being a delivery, and without being a
    /// keyboard transition either: the aggregate already had this input down.
    pub first_press: bool,
    /// Whether the keyboard state was moved by this input.
    pub keyboard_applied: bool,
    /// What a release did, when this was one.
    ///
    /// Carried rather than inferred from an absent recipient. A source that
    /// was not holding, and one whose input another source still holds, are
    /// both successful ledger outcomes that owe nobody an event -- and neither
    /// is a target that has gone.
    pub release: Option<sophia_input_authority::ReleaseOutcome>,
    /// The completion the authority recorded.
    pub completion: sophia_input_authority::RequestCompletion,
    /// Whether this outcome owes a client an event at all.
    ///
    /// Decided under the guards, where the ledger said what happened, and not
    /// inferred later from an absent event. A press that joined a hold and a
    /// release that found nothing held both legitimately owe nobody anything;
    /// an event that was owed and never built is a debt. Both look like no
    /// event afterwards, and treating them alike either strands finished work
    /// or discards an obligation.
    pub owes_event: bool,
    /// The event this owes a client, decided under the guards.
    ///
    /// `None` where nothing is owed one: a press that joined a hold moved the
    /// aggregate without being a delivery, and a release with a survivor left
    /// the aggregate unchanged. Emitting either would send a client a
    /// transition that did not happen to it.
    pub event: Option<XAuthorityInputEvent>,
}
