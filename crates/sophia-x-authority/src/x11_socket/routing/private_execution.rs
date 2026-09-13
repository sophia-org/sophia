// The ordered path from an accepted request to a delivered event.
//
// Split by subject from the admission boundary and the authority facade: this
// is what happens to one admitted input once it is runnable, and the order its
// steps happen in is the whole of it.

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
    /// The admission boundary or the authority refused.
    Authority(PrivateAuthorityRefusal),
}

/// What one ordered input did.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivateOrderedRun {
    /// Where it went, as decided under the guards.
    pub reached: PrivateReachedResources,
    /// Whether this press began the hold rather than joining one.
    ///
    /// A join moves the ledger without being a delivery, and without being a
    /// keyboard transition either: the aggregate already had this input down.
    pub first_press: bool,
    /// Whether the keyboard state was moved by this input.
    pub keyboard_applied: bool,
    /// The completion the authority recorded.
    pub completion: sophia_input_authority::RequestCompletion,
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

        let client = custody.client();
        let mut reached = None;
        let mut first_press = false;
        let mut keyboard_applied = false;
        let Self {
            participant,
            broker,
            ..
        } = self;
        let completion = participant
            .execute_current(custody, client, |permit| {
                resolve_and_apply(
                    permit,
                    &broker.registry,
                    keyboards,
                    route,
                    &mut reached,
                    &mut first_press,
                    &mut keyboard_applied,
                )
            })
            .map_err(|_| {
                PrivateExecutionRefusal::Authority(PrivateAuthorityRefusal::NoCurrentAdmission)
            })?
            .map_err(PrivateExecutionRefusal::Authority)?;

        let Some(reached) = reached else {
            // The transaction returned without deciding where this went, which
            // is a refusal recorded by the authority rather than a delivery.
            return Err(PrivateExecutionRefusal::TargetGone);
        };
        Ok(PrivateOrderedRun {
            reached,
            first_press,
            keyboard_applied,
            completion,
        })
    }
}

/// Resolve where an input goes and apply it, with common already held.
///
/// The X guards are taken here and in their own rank: surfaces, then the
/// pointer mapper, then the grab record. Nothing reaches backward for an
/// earlier-ranked guard after taking a later one, and nothing waits.
///
/// A press resolves; a release does not. A release answers to the recipient
/// the first press reached, which the ledger recorded, so asking the route
/// again would refuse exactly when the grab that chose it has gone -- which is
/// when a release matters most.
#[cfg(unix)]
fn resolve_and_apply(
    permit: &mut sophia_input_authority::ExecutionPermit<'_>,
    registry: &XServerFrontendRouteRegistry,
    keyboards: &mut PrivateKeyboards,
    route: &XAuthorityRoutedInput,
    reached: &mut Option<PrivateReachedResources>,
    first_press: &mut bool,
    keyboard_applied: &mut bool,
) -> Result<(), sophia_input_authority::RegistrationError> {
    let unavailable = sophia_input_authority::RegistrationError::RoutingUnavailable;
    match route.request.kind {
        InputEventKind::PointerButton { button, pressed } => {
            let surfaces = registry.surfaces.lock().map_err(|_| unavailable)?;
            let Some(surface_route) = surfaces.get(&route.request.target_surface).copied() else {
                return Err(unavailable);
            };
            // Named without moving anything: the ledger has to name the input
            // it is validating, and validation comes before any effect.
            let Some(core_button) = crate::XCorePointerMapper::peek_evdev_button(button) else {
                return Err(unavailable);
            };
            let input = sophia_input_authority::Input::button(
                core_button,
                sophia_input_authority::Capacity::PLANNED.button_domain(),
            )
            .map_err(|_| unavailable)?;

            if pressed {
                // Resolved here and not before. A grab taken or dropped since
                // admission changes where this goes, so a recipient chosen
                // earlier names somewhere it never reached.
                let authority = registry.input_authority.lock().map_err(|_| unavailable)?;
                let grab = authority.pointer_grab(surface_route.namespace);
                let (client, window, grabbed) = match grab {
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
                drop(authority);
                let applied = permit.press(
                    input,
                    sophia_input_authority::Recipient {
                        recipient: client.raw(),
                        connection_generation: permit.context().connection.connection_generation,
                    },
                )?;
                *first_press = applied.first_press();
                *reached = Some(PrivateReachedResources {
                    client,
                    window,
                    surface: route.request.target_surface,
                    namespace: surface_route.namespace,
                    grabbed,
                });
                // A button moves no keyboard state, so nothing is applied and
                // the report says so rather than leaving it to be assumed.
                *keyboard_applied = false;
            } else {
                let outcome = permit.release(input)?;
                let sophia_input_authority::ReleaseOutcome::DeliverTo(hold) = outcome else {
                    // Not held, or another source still holds it. The ledger
                    // moved or refused; either way nothing is owed a delivery,
                    // and inventing a recipient would answer for a hold that
                    // is not this one's to end.
                    return Ok(());
                };
                // The recipient the first press reached, taken from the hold
                // rather than resolved again.
                *reached = Some(PrivateReachedResources {
                    client: XServerFrontendClientId::from_raw(hold.recipient),
                    window: surface_route.window,
                    surface: route.request.target_surface,
                    namespace: surface_route.namespace,
                    grabbed: false,
                });
                *keyboard_applied = false;
            }
            Ok(())
        }
        InputEventKind::Key { .. } => {
            // A new press needs an authoritative reached target. The only
            // focus record available is written after the writer command is
            // queued, so it says a change was asked for, not that one was
            // applied. Delivering on it would name a client that may never
            // have received focus.
            //
            // No key hold can exist while this refuses, so a release has
            // nothing recorded to answer to and refuses with it.
            let _ = keyboards;
            let _ = keyboard_applied;
            Err(sophia_input_authority::RegistrationError::RoutingUnavailable)
        }
        // Neither names an input this ledger validates. Motion and axis carry
        // no hold, so there is nothing to press, join or release, and passing
        // them through the ordered path would record a transition that did not
        // happen.
        InputEventKind::PointerMotion | InputEventKind::PointerAxis { .. } => {
            Err(sophia_input_authority::RegistrationError::RoutingUnavailable)
        }
    }
}
