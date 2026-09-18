// The ordered path from an accepted request to a delivered event.
//
// Split by subject from the admission boundary and the authority facade: this
// is what happens to one admitted input once it is runnable, and the order its
// steps happen in is the whole of it.

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
    ) -> Result<PrivateExecutionAttempt, PrivateExecutionRefusal> {
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
        terminal.shared_activation.invalidate();
        let blocked_by_frozen = !terminal.current_is_frozen && !terminal.frozen.is_empty();
        let previous_shape = terminal.native_shape();
        let PrivateTerminalInventory {
            current,
            current_freeze,
            holds,
            settling,
            native_pending,
            pending_custody,
            next_event_order,
            transients,
            ..
        } = terminal;
        let Some(PrivateOrderedItem::Refused { custody, route, .. }) = current.as_ref() else {
            return Err(PrivateExecutionRefusal::NotAttempted);
        };
        let outcome = execute_owned(
            watched,
            native,
            native_pending,
            pending_custody,
            next_event_order,
            transients,
            controller,
            participant,
            broker,
            holds,
            settling,
            keyboards,
            route,
            custody,
            Some(current_freeze),
            blocked_by_frozen,
        );
        if previous_shape != terminal.native_shape() {
            terminal.live_disposal.invalidate();
        }
        outcome
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
        terminal.shared_activation.invalidate();
        let previous_shape = terminal.native_shape();
        let PrivateTerminalInventory {
            holds,
            settling,
            native_pending,
            pending_custody,
            next_event_order,
            transients,
            ..
        } = terminal;
        let outcome = execute_owned(
            &mut watched,
            native,
            native_pending,
            pending_custody,
            next_event_order,
            transients,
            controller,
            participant,
            broker,
            holds,
            settling,
            keyboards,
            route,
            custody,
            None,
            false,
        );
        if previous_shape != terminal.native_shape() {
            terminal.live_disposal.invalidate();
        }
        // Finished on every normal way out, refusals included. What the
        // execution decided wins over a supervisor that would not take the
        // finish: the refusal is the cause, and reporting the watch instead
        // would replace what happened with what was not recorded about it.
        match (outcome, watched.finish()) {
            (Err(refusal), _) => Err(refusal),
            (Ok(PrivateExecutionAttempt::Completed(run)), Ok(())) => Ok(run),
            (Ok(PrivateExecutionAttempt::Deferred), _) => Err(PrivateExecutionRefusal::NotAttempted),
            (Ok(_), Err(_)) => Err(PrivateExecutionRefusal::Unwatched),
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
    native_pending: &mut PrivateNativePending,
    pending_custody: &mut Option<PrivateDeliveryCustody>,
    next_event_order: &mut u64,
    transients: &mut PrivateTransientInventory,
    controller: &PrivateAuthorityController,
    participant: &PrivateAdmissionParticipant,
    broker: &XServerFrontendRouteBroker,
    holds: &mut Vec<PrivateHoldRecord>,
    settling: &mut Vec<PrivateSettlingRelease>,
    keyboards: &mut PrivateKeyboards,
    route: &XAuthorityRoutedInput,
    custody: &PrivateOutstandingRequest,
    freeze: Option<&mut Option<private_native::Freeze>>,
    blocked_by_frozen: bool,
) -> Result<PrivateExecutionAttempt, PrivateExecutionRefusal> {
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

        // An interrupted source still owns the possibly applied effect. Do
        // not execute later work over that unresolved original request.
        if transients.pending.is_some() {
            return Err(PrivateExecutionRefusal::CustodyRetained);
        }
        if matches!(route.request.kind, InputEventKind::PointerAxis {
            horizontal_v120: 0, vertical_v120: 0,
        }) {
            return Err(PrivateExecutionRefusal::Unmappable);
        }

        if route.mode == XAuthorityRoutedInputMode::StateOnly
            && (!matches!(route.request.kind, InputEventKind::Key { pressed: false, .. })
                || route.delivery.is_some())
        {
            return Err(PrivateExecutionRefusal::StateOnlyUnsupported);
        }
        if route.mode == XAuthorityRoutedInputMode::Repeat {
            return Err(PrivateExecutionRefusal::RepeatUnsupported);
        }

        // Claimed, not consulted. Accepted work waits its turn in the shared
        // order, and a delivery can end during that wait: its epoch revoked,
        // its deadline passed, its client gone. Asking whether it is still
        // current and then applying it leaves a gap between the question and
        // the effect, and a cancellation landing in that gap publishes an
        // outcome the effect then contradicts. No guard spans that gap -- the
        // ledger's own is released before this takes common and the X guards,
        // which is the rank -- so what spans it is this claim.
        let held_completion = custody.input_completion();
        let claim = match held_completion {
            Some(held) if route.delivery == Some(held.delivery) => broker.registry.input_recovery
                .claim_execution_for_held(held.delivery, &held.cell)
                .map_err(PrivateExecutionRefusal::CompletionMismatch)?,
            Some(_) => return Err(PrivateExecutionRefusal::CompletionMismatch(PrivateCompletionMismatch::Replaced)),
            // Unprepared/private-component calls did not pass through the
            // producing admission. Actual prepared ingress carries its cell.
            None => broker.registry.input_recovery.claim_execution(route.delivery),
        };
        match claim {
            ExecutionClaim::Claimed => {}
            ExecutionClaim::Ended => {
                // A recovery cancellation is not the common request outcome.
                // Complete this original reservation without consuming input;
                // unreadable/revoked common remains owned for its exact
                // cancellation/retirement path. No request is re-reserved.
                let _ = participant.execute_current(custody, custody.client(), |_, _| {
                    Err(sophia_input_authority::RegistrationError::StaleRequest)
                });
                return Err(PrivateExecutionRefusal::DeliveryEnded);
            }
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
            completion: held_completion.map(|held| &held.cell),
            applied: &applied,
        };

        let client = custody.client();
        let mut notes = PrivateTransactionNotes::new(&applied, watched);
        notes.freeze = freeze;
        let completion = participant
            .execute_current_or_defer(custody, client, |permit, bindings| {
                if blocked_by_frozen {
                    return Ok(sophia_input_authority::ExecutionDisposition::Defer);
                }
                let outcome = match route.request.kind {
                    InputEventKind::Key { .. } => resolve_and_apply_key(
                        permit, bindings, &broker.registry, holds, settling, route,
                        custody.grant(), custody.capability(), native, native_pending,
                        pending_custody, next_event_order, keyboards, &mut notes,
                    ),
                    InputEventKind::PointerMotion | InputEventKind::PointerAxis { .. } =>
                        resolve_and_apply_transient(permit, bindings, &broker.registry,
                            route, custody.grant(), native, transients, pending_custody,
                            next_event_order, &mut notes),
                    InputEventKind::PointerButton { .. } => resolve_and_apply_pointer(
                        permit, bindings, &broker.registry, holds, settling, route,
                        custody.grant(), custody.capability(), native, native_pending,
                        pending_custody, next_event_order, &mut notes,
                    ),
                };
                outcome.map(|()| if notes.deferred {
                    sophia_input_authority::ExecutionDisposition::Defer
                } else {
                    sophia_input_authority::ExecutionDisposition::Complete
                })
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
        let sophia_input_authority::RequestExecution::Completed(completion) = completion else {
            return Ok(PrivateExecutionAttempt::Deferred);
        };

        // Before the rest: these say the work should not have been applied at
        // all, rather than that applying it went wrong.
        if notes.order_exhausted {
            return Err(PrivateExecutionRefusal::OrderExhausted);
        }
        if notes.custody_retained {
            return Err(PrivateExecutionRefusal::CustodyRetained);
        }
        if notes.completion_missing {
            return Err(PrivateExecutionRefusal::CompletionMissing);
        }
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
        Ok(PrivateExecutionAttempt::Completed(PrivateOrderedRun {
            owes_event: decided.owes_event,
            reached: decided.reached,
            first_press: decided.first_press,
            keyboard_applied: decided.keyboard_applied,
            release: decided.release,
            completion,
            event: decided.event,
        }))
    }
