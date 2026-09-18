/// Resolve and apply one pointer-button transaction with common already held.
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
fn resolve_and_apply_pointer(
    permit: &mut sophia_input_authority::ExecutionPermit<'_>,
    bindings: &PrivateAdmissionBindings,
    registry: &XServerFrontendRouteRegistry,
    holds: &mut Vec<PrivateHoldRecord>,
    settling: &mut Vec<PrivateSettlingRelease>,
    route: &XAuthorityRoutedInput,
    grant: sophia_input_authority::GrantId,
    capability: sophia_input_authority::DeviceCapability,
    native: &private_native::Owner,
    native_pending: &mut PrivateNativePending,
    pending_custody: &mut Option<PrivateDeliveryCustody>,
    next_event_order: &mut u64,
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
                    // ACQUIRED BEFORE THE EFFECT, and before the record that
                    // owns the native obligation is taken out of inventory.
                    // Acquiring it in the settling initializer meant reaching
                    // for a lock while the hold was already out of storage and
                    // held only by a local, so an interruption inside that
                    // acquisition lost the obligation entirely.
                    //
                    // Fail-closed, and the two reasons are told apart. A
                    // delivery with no completion can never be answered and a
                    // ledger that could not be read establishes nothing; both
                    // refuse, and the hold stays exactly where it is.
                    // Refused rather than replaced, for the same reason the
                    // press path refuses one.
                    if pending_custody.is_some() {
                        notes.custody_retained = true;
                        return Err(sophia_input_authority::RegistrationError::StaleRequest);
                    }
                    let Some(release_delivery) = route.delivery else {
                        notes.completion_missing = true;
                        return Err(sophia_input_authority::RegistrationError::StaleRequest);
                    };
                    match registry.input_recovery.completion_for(release_delivery) {
                        // Installed before the source release for the same
                        // reason the press's is: a local across that call is
                        // one an interruption takes.
                        Ok(Some(cell)) => {
                            // Checked, not saturating: a stamp that repeats puts two
                    // events in one place, which is not an order at all.
                    let Some(next) = next_event_order.checked_add(1) else {
                        notes.order_exhausted = true;
                        return Err(sophia_input_authority::RegistrationError::StaleRequest);
                    };
                    let order = *next_event_order;
                    *next_event_order = next;
                    *pending_custody = Some(PrivateDeliveryCustody::new(order, Some(cell)));
                        }
                        Ok(None) => {
                            notes.completion_missing = true;
                            return Err(sophia_input_authority::RegistrationError::StaleRequest);
                        }
                        Err(PrivateCompletionUnreadable) => {
                            notes.recovery_unavailable = true;
                            return Err(sophia_input_authority::RegistrationError::StaleRequest);
                        }
                    }
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
                        .release(permit, hold.pointer_mut().expect("a button obligation"), route, notes.may_have_applied)
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
                                custody: pending_custody
                                    .take()
                                    .expect("custody was installed before the effect"),
                                // THE PRESS'S OWN CUSTODY, CARRIED ON. Ending
                                // the physical hold does not answer the press
                                // event or transfer its delivery: that is a
                                // different event owed to the same recipient,
                                // and it needs its own instance rather than
                                // being replaced by this release's.
                                press_custody: Some(removed.custody),
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
                            // A KNOWN NO-EVENT RESULT, DISPOSED OF EXPLICITLY.
                            // The aggregate owes nobody an event, so this
                            // executor will never deliver one for the custody
                            // it prepared and holding it would refuse every
                            // later operation for work that is finished.
                            //
                            // What this claims is only that: no event is owed
                            // from here. It says nothing about whether the
                            // delivery is answered -- its ticket is still the
                            // ledger's, and the ordinary path still owns
                            // whatever becomes of it.
                            *pending_custody = None;
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
                    .join(permit, hold.pointer().expect("a button obligation"), notes.may_have_applied)
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

            // Acquired before the effect, so a press that cannot have its
            // answer recognised refuses rather than applying one. Fail-closed
            // and named, exactly as the release path is.
            // AN OCCUPIED SLOT IS REFUSED, NOT REPLACED. A refusal that left
            // the source holding context left this custody attached to that
            // same continuation; assigning over it would drop the only handle
            // able to answer what that continuation still owes, with nothing
            // recorded about what became of it.
            if pending_custody.is_some() {
                notes.custody_retained = true;
                return Err(sophia_input_authority::RegistrationError::StaleRequest);
            }
            // A private ordered event with no delivery identity could never
            // have its answer recognised, so it is refused here rather than
            // applied. Ordinary public routing keeps its own behaviour; this
            // is the private boundary's policy for itself.
            let Some(press_delivery) = route.delivery else {
                notes.completion_missing = true;
                return Err(sophia_input_authority::RegistrationError::StaleRequest);
            };
            match registry.input_recovery.completion_for(press_delivery) {
                // INSTALLED BEFORE THE EFFECT, into storage this instance
                // already owns. Held in a local across the source call it
                // would be taken by an interruption between the effect and the
                // record, leaving the event that effect just owed with no
                // handle able to answer it.
                Ok(Some(cell)) => {
                    // Checked, not saturating: a stamp that repeats puts two
                    // events in one place, which is not an order at all.
                    let Some(next) = next_event_order.checked_add(1) else {
                        notes.order_exhausted = true;
                        return Err(sophia_input_authority::RegistrationError::StaleRequest);
                    };
                    let order = *next_event_order;
                    *next_event_order = next;
                    *pending_custody = Some(PrivateDeliveryCustody::new(order, Some(cell)));
                }
                Ok(None) => {
                    notes.completion_missing = true;
                    return Err(sophia_input_authority::RegistrationError::StaleRequest);
                }
                Err(PrivateCompletionUnreadable) => {
                    notes.recovery_unavailable = true;
                    return Err(sophia_input_authority::RegistrationError::StaleRequest);
                }
            }
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
                    native_pending.pointer_slot().map_err(|refusal| {
                        notes.native_refusal = Some(refusal);
                        unavailable
                    })?,
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
                    .pointer()
                    .map_or(surface_route.window, |hold| hold.plan().delivered_window);
                let reached = PrivateReachedResources {
                    client: XServerFrontendClientId::from_raw(incarnation.recipient),
                    window: reached_window,
                    surface: Some(route.request.target_surface),
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
                    // Acquired on the operation that created this debt, the
                    // same as a release's, and before the record that will own
                    // the obligation is anywhere but here.
                    custody: pending_custody
                        .take()
                        .expect("custody was installed before the effect"),
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
        _ => Err(sophia_input_authority::RegistrationError::StaleExecution),
    }
}
