// The ordered path from an accepted request to a delivered event.
//
// Split by subject from the admission boundary and the authority facade: this
// is what happens to one admitted input once it is runnable, and the order its
// steps happen in is the whole of it.

/// How many holds and settling releases one executor may record.
///
/// A policy chosen here, set to the planned authority's input slots because a
/// hold exists per input aggregate and that is the shape the approved plan
/// fixes. **Not** a reading of the authority actually supplied to this
/// instance -- the same distinction as the per-binding grant records. An
/// authority built larger still gets this many records here; one built smaller
/// refuses on its own capacity first.
///
/// What it does do is keep the records within this policy, enforced before the
/// effect rather than reserved after it, because reserved storage says a push
/// will not allocate and says nothing about how many pushes there can be. What
/// it does **not** do is prove that every supplied authority or carried
/// generation fits without admission backpressure, and it does not retire
/// anything: a continuation still held after its debt is settled needs an
/// owner that retires it, which a count cannot be.
///
/// The ready queue's capacity bounds neither: it bounds what one turn admits,
/// and a hold outlives the turn that began it across any number of drains and
/// refills.
#[cfg(unix)]
const PRIVATE_HOLD_RECORDS: usize = sophia_input_authority::Capacity::PLANNED.input_slots();

/// Why an ordered execution did not apply.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivateExecutionRefusal {
    /// The keyboard state offered is not this instance's.
    ForeignKeyboards,
    /// This seat has no keyboard state and one could not be built. Refused
    /// before the transaction, where refusing is still free.
    SeatUnavailable,
    /// The input does not name anything this authority can validate.
    Unmappable,
    /// A new key press needs an authoritative reached target, and the only
    /// focus record available is an intent that was queued rather than one a
    /// writer applied. Refused rather than delivered somewhere plausible.
    FocusNotApplied,
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

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Run the item this instance currently owns.
    ///
    /// The custody and the work both come from the owned slot rather than from
    /// parameters, so there is no moment where the only handle to an accepted
    /// request is a local that an unwind would take with the frame. The slot
    /// is borrowed, never emptied for the call.
    fn run_current(
        &mut self,
        keyboards: &mut PrivateKeyboards,
        watched: &mut private_watchdog::PrivateWatchedExecution,
    ) -> Result<PrivateOrderedRun, PrivateExecutionRefusal> {
        let Self {
            terminal,
            participant,
            controller,
            broker,
            native_owner,
            ..
        } = self;
        // An instance whose origin was never prepared cannot press: a hold
        // cloning an origin that does not exist is one nothing could later
        // prove anything about, and the preparation happens before any
        // producer is exposed precisely so this is not a question at
        // execution time.
        let Some(native) = native_owner.as_ref() else {
            return Err(PrivateExecutionRefusal::NativeUnprepared);
        };
        let PrivateTerminalInventory {
            current,
            holds,
            settling,
            native_pending,
            ..
        } = terminal;
        let Some(PrivateOrderedItem::Refused { custody, route, .. }) = current.as_ref() else {
            return Err(PrivateExecutionRefusal::NotAttempted);
        };
        execute_owned(
            watched,
            native,
            native_pending,
            controller,
            participant,
            broker,
            holds,
            settling,
            keyboards,
            route,
            custody,
        )
    }

    /// Run one admitted input through the ordered path.
    ///
    /// The order is the substance. Before anything is entered: the keyboard
    /// state is checked to be this instance's, and the seat it will need is
    /// prepared, because building one compiles a keymap and that must not
    /// happen where refusing has stopped being free.
    ///
    /// Then one transaction. Common is taken first, the admission bindings
    /// beneath it, and the X guards beneath those, in their own rank. The
    /// target is resolved there and not before, because a grab can be taken or
    /// dropped between admission and now, and a recipient chosen earlier names
    /// somewhere the event never reached. Final validation happens inside the
    /// authority, before the callback runs at all; the aggregate transition and
    /// any keyboard effect happen after it and under the same guards. What
    /// comes out is a decision, not a plan to decide.
    ///
    /// Nothing is emitted here. Delivery is the caller's, after the guards are
    /// gone, from the immutable record this returns.
    #[cfg_attr(not(test), allow(dead_code))]
    // Narrowed to the crate rather than widening the supervisor it takes.
    // What watches an execution is this crate's arrangement, and exporting it
    // to match an entry point would publish a type nobody outside has asked
    // to hold.
    pub(crate) fn run_ordered_input(
        &mut self,
        keyboards: &mut PrivateKeyboards,
        route: &XAuthorityRoutedInput,
        custody: &PrivateOutstandingRequest,
        watch: &private_watchdog::PrivateWatchdogOwner,
    ) -> Result<PrivateOrderedRun, PrivateExecutionRefusal> {
        // Nothing runs unwatched. A supervisor that refuses to watch is not a
        // reason to go ahead without one: the whole point of the watch is the
        // case where this call does not come back.
        let mut watched = watch
            .begin_dequeued(std::time::Instant::now())
            .map_err(|_| PrivateExecutionRefusal::Unwatched)?;
        let Self {
            terminal,
            participant,
            controller,
            broker,
            native_owner,
            ..
        } = self;
        let Some(native) = native_owner.as_ref() else {
            return Err(PrivateExecutionRefusal::NativeUnprepared);
        };
        let PrivateTerminalInventory {
            holds,
            settling,
            native_pending,
            ..
        } = terminal;
        let outcome = execute_owned(
            &mut watched,
            native,
            native_pending,
            controller,
            participant,
            broker,
            holds,
            settling,
            keyboards,
            route,
            custody,
        );
        // Finished on every normal way out, refusals included. What the
        // execution decided wins over a supervisor that would not take the
        // finish: the refusal is the cause, and reporting the watch instead
        // would replace what happened with what was not recorded about it.
        match (outcome, watched.finish()) {
            (Err(refusal), _) => Err(refusal),
            (Ok(run), Ok(())) => Ok(run),
            (Ok(_), Err(_)) => Err(PrivateExecutionRefusal::Unwatched),
        }
    }
}

/// Resolve where an input goes and apply it, with common already held.
///
/// The X guards are taken here and in their own rank: surfaces, then the
/// pointer mapper, then the grab record, and each is held across the ledger
/// transition it informs. Releasing one before applying would reopen the
/// window between deciding and recording, which is the check-then-act this
/// arrangement exists to close.
///
/// A press resolves; a release does not, and does not look at the route at
/// all. A release answers to what the first press reached, which was recorded
/// then, so a surface that has since gone or a grab that has since dropped
/// changes nothing about where it is owed.
#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn resolve_and_apply(
    permit: &mut sophia_input_authority::ExecutionPermit<'_>,
    bindings: &PrivateAdmissionBindings,
    registry: &XServerFrontendRouteRegistry,
    holds: &mut Vec<PrivateHoldRecord>,
    settling: &mut Vec<PrivateSettlingRelease>,
    route: &XAuthorityRoutedInput,
    grant: sophia_input_authority::GrantId,
    capability: sophia_input_authority::DeviceCapability,
    native: &private_native::Owner,
    native_pending: &mut Option<private_native::Hold>,
    notes: &mut PrivateTransactionNotes<'_>,
) -> Result<(), sophia_input_authority::RegistrationError> {
    let unavailable = sophia_input_authority::RegistrationError::RoutingUnavailable;
    match route.request.kind {
        InputEventKind::PointerButton { button, pressed } => {
            // Named before anything moves: the ledger has to name the input it
            // is validating, and validation precedes every effect.
            let Some(core_button) = crate::XCorePointerMapper::peek_evdev_button(button) else {
                return Err(sophia_input_authority::RegistrationError::StaleExecution);
            };
            let input = sophia_input_authority::Input::button(
                core_button,
                sophia_input_authority::Capacity::PLANNED.button_domain(),
            )
            .map_err(|_| sophia_input_authority::RegistrationError::StaleExecution)?;

            if !pressed {
                // Nothing about the route is consulted. No surfaces lookup, no
                // grab, no namespace: all of that describes where the route
                // points now, and a release is owed to where its press went.
                // Taken before the ledger moves, and held through the mapper
                // update and the decision. Taking it afterwards left the two
                // transitions in separate intervals, so a pointer writer could
                // run between the hold ending and the button being lifted.
                if settling.len() >= PRIVATE_HOLD_RECORDS {
                    // A release whose output has nowhere to be kept is a
                    // delivery nobody could later prove was owed.
                    notes.records_exhausted = true;
                    return Err(sophia_input_authority::RegistrationError::Capacity(
                        sophia_input_authority::CapacityError::NoCompletionCell,
                    ));
                }
                // A release is owed to where its press went, so the hold this
                // executor retained selects the operation. The route says
                // where the pointer points now, which is a different question.
                //
                // No pointer guard is taken here: the source takes the mapper,
                // the authority and the recipient's selections together under
                // this connection, and one held over that call would be the
                // same mutex twice.
                if let Some(index) = holds.iter().position(|record| {
                    record
                        .native
                        .as_ref()
                        .is_some_and(|hold| hold.input() == input)
                }) {
                    // The exact connection the press retained. A release
                    // converts its coordinates against the geometry that
                    // connection still holds; the press's own numbers describe
                    // a moment that has passed.
                    let connection = holds[index]
                        .native
                        .as_ref()
                        .expect("selected by the hold it carries")
                        .connection();
                    let mut guards = native.lock_for_release(&connection).map_err(|refusal| {
                        notes.native_refusal = Some(refusal);
                        unavailable
                    })?;
                    // Told before the effect and again once it is known to have
                    // happened, from the same two points that decide whether a
                    // cancellation had anything to contradict.
                    notes
                        .watched
                        .applying()
                        .map_err(|_| sophia_input_authority::RegistrationError::StaleExecution)?;
                    // LENT, NOT SURRENDERED. The hold stays in the record this
                    // executor owns for the whole source release, so every
                    // residual it records -- a retained activation, a mapper
                    // that has gone, a selection no longer there -- stays
                    // attached to the obligation that owns it.
                    let hold = holds[index]
                        .native
                        .as_mut()
                        .expect("selected by the hold it carries");
                    let (outcome, built) = guards
                        .release(permit, hold, route, notes.may_have_applied)
                        .map_err(|refusal| {
                            notes.native_refusal = Some(refusal);
                            unavailable
                        })?;
                    notes
                        .watched
                        .committed()
                        .map_err(|_| sophia_input_authority::RegistrationError::StaleExecution)?;
                    // The adapter guards go here. NOTHING RECORDS A PROOF IN
                    // THIS FUNCTION: the proof enters common as its own origin,
                    // and the common transaction around this call has not
                    // dropped yet. That recording happens once it has.
                    drop(guards);
                    match outcome {
                        sophia_input_authority::ReleaseOutcome::DeliverTo(incarnation) => {
                            if holds[index].incarnation != incarnation {
                                // The ledger ended an incarnation this record
                                // is not for. The hold stays exactly where it
                                // is rather than being released against the
                                // wrong identity.
                                notes.plan_missing = true;
                                return Err(sophia_input_authority::RegistrationError::StaleRequest);
                            }
                            let reached = holds[index].reached;
                            // Kept as built or as the cause it failed with.
                            // An event that could not be built and an event
                            // that was never owed are different facts, and
                            // flattening them here would leave whoever reads
                            // the release later unable to tell which happened.
                            let (event, unbuilt) = match built {
                                Ok(event) => (event.map(XAuthorityInputEvent::Pointer), None),
                                Err(cause) => (None, Some(cause)),
                            };
                            // Bound after the ledger moved, which is the
                            // opposite of the press and for the opposite
                            // reason. The aggregate hold has already ended,
                            // and that is not conditional on whether anyone is
                            // still there to be told.
                            //
                            // NOT that the button is lifted natively. A
                            // release ending in a residual -- a mapper that
                            // has gone, for one -- leaves exactly that fact
                            // unresolved, which is why the residual is kept.
                            // What is settled here is the ledger transition,
                            // not the projection. What the binding decides here is only
                            // whether an event is owed -- and binding it to
                            // where the press went, rather than to whatever
                            // the release's own route names, is what makes a
                            // later disconnect answer it.
                            let binding =
                                match registry.input_recovery.bind(route.delivery, reached.client) {
                                    Ok(true) => PrivateReleaseBinding::Reached,
                                    Ok(false) => PrivateReleaseBinding::Ended,
                                    Err(_) => {
                                        // Nothing is emitted, but the debt is
                                        // recorded below first: a release whose
                                        // recipient nobody could look up is
                                        // still a release that happened. Kept
                                        // apart from Ended, because this
                                        // establishes nothing about whether a
                                        // receipt can still arrive.
                                        notes.recovery_unavailable = true;
                                        PrivateReleaseBinding::Unknown
                                    }
                                };
                            let reaches = binding == PrivateReleaseBinding::Reached;
                            // Moved WITH ITS SOURCE OBLIGATION. The hold owns
                            // the implicit activation, the query scope and the
                            // selection this press raised, and it is the only
                            // thing holding the exact connection they belong
                            // to. A record removed without it leaves all three
                            // owed by nobody and unretirable.
                            let removed = holds.remove(index);
                            settling.push(PrivateSettlingRelease {
                                incarnation: removed.incarnation,
                                reached: removed.reached,
                                pending: None,
                                dispatch: PrivateDispatchPhase::Untaken,
                                attempt: None,
                                // ACQUIRED HERE, on the accepted operation
                                // that created this debt, while the delivery
                                // that carries it is still the one this
                                // release was decided for. Acquiring it later
                                // meant looking the delivery up again by its
                                // id, and an id is exactly what a prune and a
                                // re-admission make unreliable.
                                completion: route
                                    .delivery
                                    .and_then(|delivery| {
                                        registry.input_recovery.completion_of(delivery)
                                    }),
                                outcome_seen: None,
                                native: removed.native,
                                unbuilt,
                                native_recorded: false,
                                native_failure: None,
                                native_attempts: 0,
                                outcome,
                                event,
                                binding,
                                delivery: route.delivery,
                            });
                            notes.decided = Some(PrivateOrderedDecision {
                                owes_event: reaches,
                                reached: Some(reached),
                                first_press: false,
                                keyboard_applied: false,
                                release: Some(outcome),
                                event: reaches.then_some(event).flatten(),
                            });
                        }
                        // Not a delivery and not a failure. The source was not
                        // holding, or another still is, so the aggregate owes
                        // nobody an event and its buttons are unchanged. The
                        // record and its obligation stay exactly as they were,
                        // and no settling entry is made -- which is what tells
                        // this apart from a delivery whose event went unbuilt.
                        sophia_input_authority::ReleaseOutcome::NotHeld
                        | sophia_input_authority::ReleaseOutcome::SurvivorRemains => {
                            notes.decided = Some(PrivateOrderedDecision {
                                owes_event: false,
                                reached: None,
                                first_press: false,
                                keyboard_applied: false,
                                release: Some(outcome),
                                event: None,
                            });
                        }
                    }
                    return Ok(());
                }

                // No hold here for this input. The ledger is still asked,
                // because a release of nothing held is an outcome and not a
                // missing target -- answering it from this executor's own
                // emptiness would be deciding what only the ledger can say.
                notes
                    .watched
                    .applying()
                    .map_err(|_| sophia_input_authority::RegistrationError::StaleExecution)?;
                notes.may_have_applied.set(true);
                let outcome = permit.release(input)?;
                notes
                    .watched
                    .committed()
                    .map_err(|_| sophia_input_authority::RegistrationError::StaleExecution)?;
                if matches!(outcome, sophia_input_authority::ReleaseOutcome::DeliverTo(_)) {
                    notes.plan_missing = true;
                    // The ledger ended a hold and the record of where it went
                    // is gone. Owing nobody an event and being unable to say
                    // who is owed one are different facts, and reporting the
                    // second as the first settles a debt by losing the
                    // evidence of it.
                    return Err(sophia_input_authority::RegistrationError::StaleRequest);
                }
                notes.decided = Some(PrivateOrderedDecision {
                    owes_event: false,
                    reached: None,
                    first_press: false,
                    keyboard_applied: false,
                    release: Some(outcome),
                    event: None,
                });
                return Ok(());
            }

            // The rank continues from the guards this transaction already
            // holds -- common, and the boundary's bindings beneath it -- with
            // clients, then surfaces, then the native base guards (pointer,
            // then X authority), and the recipient's exact selections taken
            // inside the source operation itself.
            //
            // The surface route is read under a guard that stays held through
            // resolution and application. Losing the mapper and grab locks
            // from this function does not make the route safe to read and
            // release: what it names has to still be true when the effect
            // lands, and a route read and let go describes a moment that has
            // passed.
            // A KNOWN JOIN IS ASKED AS A JOIN. This executor already holds the
            // obligation for this input, so the source has the operation for
            // it and nothing here needs resolving: no surfaces, no current
            // selection, no grab recipient lookup. Asking press instead would
            // install a second native obligation, leave it retained in pending
            // on the disagreement the source reports, and refuse every later
            // press of this instance for a phase it put there itself.
            //
            // This selects which operation to ask, and does not decide
            // first_press. Guards::join enters permit.press and refuses unless
            // the ledger agrees it is a join of exactly this incarnation.
            if let Some(index) = holds.iter().position(|record| {
                record
                    .native
                    .as_ref()
                    .is_some_and(|hold| hold.input() == input)
            }) {
                let record = &holds[index];
                let hold = record
                    .native
                    .as_ref()
                    .expect("selected by the hold it carries");
                // Bound to the recipient the press reached, before the ledger
                // is entered. Re-resolving would bind this delivery to
                // whoever the route reaches now, and a grab taken since the
                // press makes those two different clients.
                match registry.input_recovery.bind(route.delivery, hold.client()) {
                    Ok(true) => {}
                    Ok(false) => {
                        notes.delivery_ended = true;
                        return Err(sophia_input_authority::RegistrationError::StaleRequest);
                    }
                    Err(_) => {
                        notes.recovery_unavailable = true;
                        return Err(sophia_input_authority::RegistrationError::StaleRequest);
                    }
                }
                // The exact connection the hold retained, not whichever
                // connection this client has now.
                let connection = hold.connection();
                let mut guards = native.lock_for_release(&connection).map_err(|refusal| {
                    notes.native_refusal = Some(refusal);
                    unavailable
                })?;
                notes
                    .watched
                    .applying()
                    .map_err(|_| sophia_input_authority::RegistrationError::StaleExecution)?;
                // The hold is lent, never surrendered. A disagreement leaves
                // it exactly where it was, with the work already accepted
                // still owned here rather than replaced by a synthetic one.
                let applied = guards
                    .join(permit, hold, notes.may_have_applied)
                    .map_err(|refusal| {
                        notes.native_refusal = Some(refusal);
                        unavailable
                    })?;
                notes
                    .watched
                    .committed()
                    .map_err(|_| sophia_input_authority::RegistrationError::StaleExecution)?;
                drop(guards);
                debug_assert!(
                    !applied.first_press(),
                    "the source refuses a join that is not one"
                );
                notes.decided = Some(PrivateOrderedDecision {
                    // The button is already down. A join owes nobody an event.
                    owes_event: false,
                    reached: Some(record.reached),
                    first_press: false,
                    keyboard_applied: false,
                    release: None,
                    event: None,
                });
                return Ok(());
            }

            let clients = registry.clients.lock().map_err(|_| unavailable)?;
            let surfaces = registry.surfaces.lock().map_err(|_| unavailable)?;
            let Some(surface_route) = surfaces.get(&route.request.target_surface).copied() else {
                return Err(unavailable);
            };
            // Checked before anything moves, against storage reserved before
            // any work was accepted. That is what lets the hold this press may
            // begin be recorded by a push which cannot grow the vector, so
            // moving the native obligation out of pending afterwards follows
            // the only fallible step rather than preceding it.
            if holds.len() >= PRIVATE_HOLD_RECORDS {
                notes.records_exhausted = true;
                return Err(sophia_input_authority::RegistrationError::Capacity(
                    sophia_input_authority::CapacityError::NoGrantSlot,
                ));
            }
            // Whether this press joins a hold this executor already has, taken
            // by value: the press may push a new record, and a borrow held
            // across that would have to be surrendered exactly where the
            // decision is needed.
            let joining = holds
                .iter()
                .find(|record| record.incarnation.input == input)
                .map(|record| (record.incarnation, record.reached));

            let mut guards = native.lock_base().map_err(|refusal| {
                notes.native_refusal = Some(refusal);
                unavailable
            })?;
            // The fallback the source uses when no grab is established. Its
            // mask is not selection authority: a new implicit activation is
            // refined against what the recipient actually selected, so this
            // names an owner and a window and claims nothing about what may be
            // delivered through it.
            let implicit = crate::XActiveInputGrab {
                owner: surface_route.client.raw(),
                window: surface_route.window,
                owner_events: true,
                pointer_mode: 1,
                keyboard_mode: 1,
                event_mask: u16::MAX,
                xi_event_mask: [0; 8],
                xi_event_mask_words: 0,
                route_lease: route.route_lease,
            };

            notes
                .watched
                .applying()
                .map_err(|_| sophia_input_authority::RegistrationError::StaleExecution)?;
            let (applied, event) = guards
                .press(
                    permit,
                    capability,
                    route,
                    surface_route.window,
                    implicit,
                    native_pending,
                    notes.may_have_applied,
                    |recipient| {
                        // The recipient's own binding, read from the boundary
                        // this transaction already holds. A recipient with no
                        // binding is not admitted here, which is a fact about
                        // the boundary rather than about the route.
                        let binding = bindings
                            .bound
                            .get(&recipient)
                            .ok_or(PrivateAppliedRegistryRefusal::MissingAdmission)?;
                        registry.applied_client(&clients, recipient, binding)
                    },
                    |witness, selected, prepared, event| {
                        // Where this press actually reaches, from the source's
                        // own selection rather than from the route: a grab
                        // sends it elsewhere, and owner_events sends it back to
                        // the surface's own window.
                        let grab = prepared.recipient();
                        let recipient = XServerFrontendClientId::from_raw(grab.owner);
                        let window = if grab.owner_events && recipient == surface_route.client {
                            surface_route.window
                        } else {
                            grab.window
                        };
                        witness
                            .lock_publication()
                            .map_err(|_| PrivateAppliedRefusal::Interrupted)?
                            .view(recipient, selected, prepared.authority())?
                            .pointer(
                                window,
                                *event,
                                None,
                                PrivatePointerSelection::Prepared(prepared),
                            )
                    },
                )
                .map_err(|refusal| {
                    // Carried out under its own name. The source distinguishes
                    // a delivery that ended, a ledger nobody could read, a
                    // selection that was not there and an origin that was not
                    // ours, and renaming any of those to an authority error
                    // would lose which one happened.
                    notes.native_refusal = Some(refusal);
                    sophia_input_authority::RegistrationError::StaleExecution
                })?;
            notes
                .watched
                .committed()
                .map_err(|_| sophia_input_authority::RegistrationError::StaleExecution)?;
            let incarnation = applied.incarnation();
            let reached = if applied.first_press() {
                // Where the press reached, taken from what the ledger minted
                // and what the source resolved, rather than from the route
                // this executor was handed. The recipient is the incarnation's
                // own; the window is the one the resolution delivered to.
                let reached_window = native_pending
                    .as_ref()
                    .map_or(surface_route.window, |hold| hold.plan().delivered_window);
                let reached = PrivateReachedResources {
                    client: XServerFrontendClientId::from_raw(incarnation.recipient),
                    window: reached_window,
                    surface: route.request.target_surface,
                    namespace: surface_route.namespace,
                    seat: route.request.seat,
                    grant,
                };
                // Pushed into storage reserved before anything was accepted,
                // so recording where the press went cannot fail after the
                // ledger has already moved -- and the native obligation leaves
                // pending only once a record exists to hold it.
                holds.push(PrivateHoldRecord {
                    incarnation,
                    reached,
                    native: None,
                });
                holds.last_mut().expect("just pushed").native = native_pending.take();
                if joining.is_some() {
                    // The ledger began a hold for an input this executor
                    // already had one for. The record above keeps the new hold
                    // from being hidden, but the delivery was bound on the
                    // strength of a reading the ledger did not share.
                    notes.plan_missing = true;
                    return Err(sophia_input_authority::RegistrationError::StaleRequest);
                }
                Some(reached)
            } else {
                // THE LEDGER DISAGREES. This path was entered as a new press,
                // because no record here carries a native hold for this input,
                // and the ledger has answered that the button was already
                // down. One of the two readings is wrong and this executor is
                // not the one that can say which.
                //
                // The source installed an obligation before entering the
                // ledger and left it retained on this disagreement. It stays
                // in pending: it names real native state, and dropping it
                // because it arrived unexpectedly would leave an activation,
                // a query scope and a selection owed by nobody.
                notes.plan_missing = true;
                return Err(sophia_input_authority::RegistrationError::StaleRequest);
            };
            let event = event.map(XAuthorityInputEvent::Pointer);
            notes.decided = Some(PrivateOrderedDecision {
                owes_event: applied.first_press(),
                reached,
                first_press: applied.first_press(),
                keyboard_applied: false,
                release: None,
                event,
            });
            Ok(())
        }
        // Refused before the transaction was entered, with their own causes.
        // Reaching here would mean something admitted a kind this path does
        // not run.
        InputEventKind::Key { .. }
        | InputEventKind::PointerMotion
        | InputEventKind::PointerAxis { .. } => {
            Err(sophia_input_authority::RegistrationError::StaleExecution)
        }
    }
}

/// Execute one admitted input against pieces the caller already owns.
///
/// Takes the parts rather than the whole instance so the custody can be
/// borrowed from the slot that owns it while the rest is used mutably.
#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn execute_owned(
    watched: &mut private_watchdog::PrivateWatchedExecution,
    native: &private_native::Owner,
    native_pending: &mut Option<private_native::Hold>,
    controller: &PrivateAuthorityController,
    participant: &PrivateAdmissionParticipant,
    broker: &XServerFrontendRouteBroker,
    holds: &mut Vec<PrivateHoldRecord>,
    settling: &mut Vec<PrivateSettlingRelease>,
    keyboards: &mut PrivateKeyboards,
    route: &XAuthorityRoutedInput,
    custody: &PrivateOutstandingRequest,
) -> Result<PrivateOrderedRun, PrivateExecutionRefusal> {
        let identity = controller
            .identity()
            .map_err(PrivateExecutionRefusal::Authority)?;
        if !keyboards.answers_for(identity) {
            // Another instance's history would answer every identity check the
            // request can make and hold none of the keys this one is holding.
            return Err(PrivateExecutionRefusal::ForeignKeyboards);
        }
        if !keyboards.prepare(route.request.seat) {
            return Err(PrivateExecutionRefusal::SeatUnavailable);
        }

        // Refused before the transaction, so each reason is its own. Deciding
        // these inside would mean borrowing an authority error to stand for a
        // question the authority was never asked -- and a caller acting on a
        // release barrier that is really an unapplied focus looks in entirely
        // the wrong place.
        match route.request.kind {
            InputEventKind::PointerButton { .. } => {}
            InputEventKind::Key { .. } => return Err(PrivateExecutionRefusal::FocusNotApplied),
            InputEventKind::PointerMotion | InputEventKind::PointerAxis { .. } => {
                return Err(PrivateExecutionRefusal::Unmappable);
            }
        }

        // Claimed, not consulted. Accepted work waits its turn in the shared
        // order, and a delivery can end during that wait: its epoch revoked,
        // its deadline passed, its client gone. Asking whether it is still
        // current and then applying it leaves a gap between the question and
        // the effect, and a cancellation landing in that gap publishes an
        // outcome the effect then contradicts. No guard spans that gap -- the
        // ledger's own is released before this takes common and the X guards,
        // which is the rank -- so what spans it is this claim.
        match broker.registry.input_recovery.claim_execution(route.delivery) {
            ExecutionClaim::Claimed => {}
            ExecutionClaim::Ended => return Err(PrivateExecutionRefusal::DeliveryEnded),
            ExecutionClaim::Contended => {
                return Err(PrivateExecutionRefusal::DeliveryClaimedElsewhere);
            }
            ExecutionClaim::Unavailable => {
                return Err(PrivateExecutionRefusal::RecoveryUnavailable);
            }
        }
        // From here every path gives the claim back, including an unwind. A
        // claim nobody resolves is a delivery nobody can cancel again.
        // Written where the progress happens and read where the claim is given
        // back, rather than copied between the two. Everything after a
        // fallible call is skipped when that call returns an error, and this
        // marker matters most exactly then: a refusal that never reached an
        // effect is what makes a deferred cancellation stand.
        let applied = std::cell::Cell::new(false);
        let _claim = PrivateDeliveryClaim {
            recovery: &broker.registry.input_recovery,
            delivery: route.delivery,
            applied: &applied,
        };

        let client = custody.client();
        let mut notes = PrivateTransactionNotes::new(&applied, watched);
        let completion = participant
            .execute_current(custody, client, |permit, bindings| {
                resolve_and_apply(
                    permit,
                    bindings,
                    &broker.registry,
                    holds,
                    settling,
                    route,
                    custody.grant(),
                    custody.capability(),
                    native,
                    native_pending,
                    &mut notes,
                )
            })
            // Typed through, not collapsed. An unreadable boundary is not a
            // client nobody admitted, and saying so here would reinstate the
            // conflation that was already repaired one level down.
            .map_err(|refusal| match refusal {
                PrivateAdmissionRefusal::Unreachable => {
                    PrivateExecutionRefusal::Authority(PrivateAuthorityRefusal::Unreachable)
                }
                other => PrivateExecutionRefusal::Admission(other),
            })?
            .map_err(PrivateExecutionRefusal::Authority)?;

        // Before the rest: these say the work should not have been applied at
        // all, rather than that applying it went wrong.
        if notes.recovery_unavailable {
            return Err(PrivateExecutionRefusal::RecoveryUnavailable);
        }
        if notes.delivery_ended {
            return Err(PrivateExecutionRefusal::DeliveryEnded);
        }
        if notes.records_exhausted {
            return Err(PrivateExecutionRefusal::RecordsExhausted);
        }
        if notes.plan_missing {
            // Named for what it is rather than by whatever authority error
            // carried it out of the transaction. A hold ended and its record
            // is gone, which is an obligation nobody can currently discharge.
            return Err(PrivateExecutionRefusal::HoldPlanMissing);
        }
        if let Some(refusal) = notes.native_refusal {
            // Named by the source rather than by the authority error that
            // carried it out of the transaction, for the same reason the
            // refusals above are: the error says the transaction did not
            // complete, and the refusal says what stopped it.
            return Err(PrivateExecutionRefusal::Native(refusal));
        }
        let Some(decided) = notes.decided else {
            // The transaction returned without deciding anything, which means
            // the callback refused before recording. Its own cause travelled
            // in the completion rather than being renamed here.
            return Err(PrivateExecutionRefusal::NotDecided(completion));
        };
        Ok(PrivateOrderedRun {
            owes_event: decided.owes_event,
            reached: decided.reached,
            first_press: decided.first_press,
            keyboard_applied: decided.keyboard_applied,
            release: decided.release,
            completion,
            event: decided.event,
        })
    }
