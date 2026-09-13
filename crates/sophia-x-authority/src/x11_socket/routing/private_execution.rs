// The ordered path from an accepted request to a delivered event.
//
// Split by subject from the admission boundary and the authority facade: this
// is what happens to one admitted input once it is runnable, and the order its
// steps happen in is the whole of it.

/// How many holds and settling releases one executor may record.
///
/// The authority's own input slots, which is what actually bounds them: a hold
/// exists per input aggregate, so at most this many can be held at once and at
/// most this many can be awaiting settlement behind them. The ready queue's
/// capacity does not bound either -- it bounds what is admitted in one turn,
/// and a hold outlives the turn that began it across any number of drains and
/// refills.
///
/// Enforced before the effect, not merely reserved. Reserved storage says a
/// push will not allocate; it says nothing about how many pushes there can be.
#[cfg(unix)]
const PRIVATE_HOLD_RECORDS: usize = sophia_input_authority::Capacity::PLANNED.input_slots();

/// What one ordered input reached.
///
/// Decided once, under the guards that decide it, and never asked again. The
/// fields are private and there is no way to build one outside the resolver,
/// so a later step cannot revise where an event went after the ledger has
/// recorded it going there.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivateReachedResources {
    client: XServerFrontendClientId,
    window: XResourceId,
    surface: SurfaceId,
    namespace: NamespaceId,
    /// The seat whose pointer state this hold moved.
    ///
    /// Recorded with the plan so the release moves the same mapper the press
    /// moved. Finding it from the current route or from whatever seat a
    /// release happens to name would clear a different seat's buttons and
    /// leave this one's held forever.
    seat: SeatId,
    /// Whether a grab chose this rather than the route.
    grabbed: bool,
}

#[cfg(unix)]
impl PrivateReachedResources {
    pub fn client(self) -> XServerFrontendClientId {
        self.client
    }
    pub fn window(self) -> XResourceId {
        self.window
    }
    pub fn surface(self) -> SurfaceId {
        self.surface
    }
    pub fn namespace(self) -> NamespaceId {
        self.namespace
    }
    pub fn seat(self) -> SeatId {
        self.seat
    }
    pub fn grabbed(self) -> bool {
        self.grabbed
    }
}

/// Why an ordered execution did not apply.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateExecutionRefusal {
    /// The keyboard state offered is not this instance's.
    ForeignKeyboards,
    /// This seat has no keyboard state and one could not be built. Refused
    /// before the transaction, where refusing is still free.
    SeatUnavailable,
    /// The route names a surface nothing currently routes.
    TargetGone,
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
    /// The transaction returned without deciding.
    ///
    /// Carries the completion the authority actually recorded, because that is
    /// the cause. Discarding it and naming a plausible error here would
    /// replace what happened with a guess about it.
    NotDecided(sophia_input_authority::RequestCompletion),
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
    pub fn run_ordered_input(
        &mut self,
        keyboards: &mut PrivateKeyboards,
        route: &XAuthorityRoutedInput,
        custody: &PrivateOutstandingRequest,
    ) -> Result<PrivateOrderedRun, PrivateExecutionRefusal> {
        let identity = self
            .controller
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

        let client = custody.client();
        let mut decided = None;
        let mut plan_missing = false;
        let mut records_exhausted = false;
        let Self {
            participant,
            broker,
            holds,
            settling,
            ..
        } = self;
        let completion = participant
            .execute_current(custody, client, |permit, bindings| {
                resolve_and_apply(
                    permit,
                    bindings,
                    &broker.registry,
                    holds,
                    settling,
                    route,
                    &mut decided,
                    &mut plan_missing,
                    &mut records_exhausted,
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

        if records_exhausted {
            return Err(PrivateExecutionRefusal::RecordsExhausted);
        }
        if plan_missing {
            // Named for what it is rather than by whatever authority error
            // carried it out of the transaction. A hold ended and its record
            // is gone, which is an obligation nobody can currently discharge.
            return Err(PrivateExecutionRefusal::HoldPlanMissing);
        }
        let Some(decided) = decided else {
            // The transaction returned without deciding anything, which means
            // the callback refused before recording. Its own cause travelled
            // in the completion rather than being renamed here.
            return Err(PrivateExecutionRefusal::NotDecided(completion));
        };
        Ok(PrivateOrderedRun {
            reached: decided.reached,
            first_press: decided.first_press,
            keyboard_applied: decided.keyboard_applied,
            release: decided.release,
            completion,
            event: decided.event,
        })
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
    holds: &mut Vec<(u64, PrivateReachedResources)>,
    settling: &mut Vec<PrivateSettlingRelease>,
    route: &XAuthorityRoutedInput,
    decided: &mut Option<PrivateOrderedDecision>,
    plan_missing: &mut bool,
    records_exhausted: &mut bool,
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
                    *records_exhausted = true;
                    return Err(sophia_input_authority::RegistrationError::Capacity(
                        sophia_input_authority::CapacityError::NoCompletionCell,
                    ));
                }
                let mut pointers = registry.pointer_state.lock().map_err(|_| unavailable)?;
                let outcome = permit.release(input)?;
                match outcome {
                    sophia_input_authority::ReleaseOutcome::DeliverTo(hold) => {
                        let Some(index) = holds.iter().position(|(id, _)| *id == hold.hold())
                        else {
                            *plan_missing = true;
                            // The ledger ended a hold and the record of where
                            // it went is gone. Owing nobody an event and being
                            // unable to say who is owed one are different
                            // facts, and reporting the second as the first
                            // settles a debt by losing the evidence of it.
                            return Err(sophia_input_authority::RegistrationError::StaleRequest);
                        };
                        let reached = holds[index].1;
                        // The mapper the press moved, keyed by what the press
                        // recorded. A release naming its own seat, or found
                        // from the current route, would clear a different
                        // seat's buttons and leave this one's held forever.
                        //
                        // Not created if absent. A press projected this
                        // button, so a missing mapper is retained state that
                        // has become unavailable, and a fresh one would be a
                        // clear history asserting the button was never down.
                        let Some(pointer) = pointers.get_mut(&(reached.namespace, reached.seat))
                        else {
                            return Err(unavailable);
                        };
                        // Moved only on a final release, and the state it
                        // reports is the one before this event -- which still
                        // has this button down, because this is the event that
                        // lifts it.
                        let event = pointer.map_evdev_button(button, false).map(
                            |(core, before)| {
                                XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                                    kind: XAuthorityPointerEventKind::Button {
                                        button: core,
                                        pressed: false,
                                    },
                                    surface: reached.surface,
                                    root_x: clamp_input_coordinate(
                                        route.request.global_position.x,
                                    ),
                                    root_y: clamp_input_coordinate(
                                        route.request.global_position.y,
                                    ),
                                    event_x: clamp_input_coordinate(
                                        route.request.local_position.x,
                                    ),
                                    event_y: clamp_input_coordinate(
                                        route.request.local_position.y,
                                    ),
                                    state: before,
                                    time_msec: u32::try_from(route.request.time_msec)
                                        .unwrap_or(u32::MAX),
                                })
                            },
                        );
                        // Moved to the continuation rather than deleted, and
                        // with everything the delivery owes rather than the
                        // plan alone. An event having been built is not an
                        // event having been delivered, and reconstructing its
                        // coordinates or its state from later facts would
                        // describe a different moment.
                        let (id, plan) = holds.remove(index);
                        settling.push(PrivateSettlingRelease {
                            hold: id,
                            reached: plan,
                            outcome,
                            event,
                        });
                        *decided = Some(PrivateOrderedDecision {
                            reached: Some(reached),
                            first_press: false,
                            keyboard_applied: false,
                            release: Some(outcome),
                            event,
                        });
                    }
                    // Not a delivery and not a failure. The source was not
                    // holding, or another still is, so the aggregate owes
                    // nobody an event and its buttons are unchanged: moving
                    // the mapper here would lift a button somebody still
                    // holds.
                    sophia_input_authority::ReleaseOutcome::NotHeld
                    | sophia_input_authority::ReleaseOutcome::SurvivorRemains => {
                        *decided = Some(PrivateOrderedDecision {
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

            let surfaces = registry.surfaces.lock().map_err(|_| unavailable)?;
            let Some(surface_route) = surfaces.get(&route.request.target_surface).copied() else {
                return Err(unavailable);
            };
            // Taken after surfaces, in rank, and both held across the ledger
            // transition below.
            let mut pointers = registry.pointer_state.lock().map_err(|_| unavailable)?;
            let pointer = pointers
                .entry((surface_route.namespace, route.request.seat))
                .or_insert_with(crate::XCorePointerMapper::new);
            // Held across the press, not read and released. A grab writer can
            // take this without holding common, so letting go before applying
            // reopens exactly the window resolving here was meant to close.
            let grabs = registry.input_authority.lock().map_err(|_| unavailable)?;
            let (client, window, grabbed) = match grabs.pointer_grab(surface_route.namespace) {
                Some(grab) => {
                    let owner = XServerFrontendClientId::from_raw(grab.owner);
                    let window = if grab.owner_events && owner == surface_route.client {
                        surface_route.window
                    } else {
                        grab.window
                    };
                    (owner, window, true)
                }
                None => (surface_route.client, surface_route.window, false),
            };
            // Checked before the ledger moves. A press whose plan could not be
            // recorded would leave a hold nobody can later answer, and
            // refusing after the effect is refusing too late.
            if holds.len() >= PRIVATE_HOLD_RECORDS {
                *records_exhausted = true;
                return Err(sophia_input_authority::RegistrationError::Capacity(
                    sophia_input_authority::CapacityError::NoGrantSlot,
                ));
            }
            // The recipient's own admission, read from the binding under the
            // guard already held. The submitting request's generation says who
            // sent this and nothing about a different client receiving it.
            let Some(recipient) = bindings.recipient(client) else {
                return Err(sophia_input_authority::RegistrationError::WrongConnection);
            };

            let applied = permit.press(input, recipient)?;
            let hold = applied.incarnation().hold();
            let reached = if applied.first_press() {
                let reached = PrivateReachedResources {
                    client,
                    window,
                    surface: route.request.target_surface,
                    namespace: surface_route.namespace,
                    seat: route.request.seat,
                    grabbed,
                };
                // Published into storage reserved before anything was
                // accepted, so recording where the press went cannot fail
                // after the ledger has already moved.
                holds.push((hold, reached));
                Some(reached)
            } else {
                // A join adopts the hold that already exists. What this press
                // would have resolved is a proposal the ledger did not take,
                // and reporting it would name a client the hold never went to.
                let Some((_, reached)) = holds.iter().find(|(id, _)| *id == hold) else {
                    // The ledger joined a hold whose record is gone, so this
                    // press has an owner nobody can name.
                    *plan_missing = true;
                    return Err(sophia_input_authority::RegistrationError::StaleRequest);
                };
                Some(*reached)
            };
            let event = if applied.first_press() {
                pointer
                    .map_evdev_button(button, true)
                    .map(|(core, before)| {
                        XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                            kind: XAuthorityPointerEventKind::Button {
                                button: core,
                                pressed: true,
                            },
                            surface: route.request.target_surface,
                            root_x: clamp_input_coordinate(route.request.global_position.x),
                            root_y: clamp_input_coordinate(route.request.global_position.y),
                            event_x: clamp_input_coordinate(route.request.local_position.x),
                            event_y: clamp_input_coordinate(route.request.local_position.y),
                            state: before,
                            time_msec: u32::try_from(route.request.time_msec).unwrap_or(u32::MAX),
                        })
                    })
            } else {
                // A join moves the aggregate without being a delivery, so the
                // pointer state is not moved either: it already has this down.
                None
            };
            *decided = Some(PrivateOrderedDecision {
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

/// A release whose delivery has been decided and not yet handed on.
///
/// Everything the delivery owes, kept together and bound to the hold it ends.
/// The plan alone is not enough: the event carries the coordinates and the
/// state from the moment it was decided, and rebuilding either from later
/// facts would describe a different moment.
#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
pub struct PrivateSettlingRelease {
    hold: u64,
    reached: PrivateReachedResources,
    outcome: sophia_input_authority::ReleaseOutcome,
    event: Option<XAuthorityInputEvent>,
}

#[cfg(unix)]
impl PrivateSettlingRelease {
    pub fn hold(self) -> u64 {
        self.hold
    }
    pub fn reached(self) -> PrivateReachedResources {
        self.reached
    }
    pub fn outcome(self) -> sophia_input_authority::ReleaseOutcome {
        self.outcome
    }
    pub fn event(self) -> Option<XAuthorityInputEvent> {
        self.event
    }
}

/// What the guarded transition decided, before anything is emitted.
#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
struct PrivateOrderedDecision {
    reached: Option<PrivateReachedResources>,
    first_press: bool,
    keyboard_applied: bool,
    release: Option<sophia_input_authority::ReleaseOutcome>,
    event: Option<XAuthorityInputEvent>,
}
