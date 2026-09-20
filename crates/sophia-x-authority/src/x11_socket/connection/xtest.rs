// What one admitted connection may do to the seat, and how it does it.
//
// Held by the connection loop beside its other per-connection state, and
// present only when an injector was issued at setup. Everything here runs
// with the runtime guard released: submitting to the authority and waiting
// for its completion are both things that guard must not be held across, so
// the dispatcher validates and refuses under the guard, and this acts after.

/// The fields of a FakeInput the loop keeps from the decode, so it can act on
/// an accepted request after the guard that validated it has been released.
#[derive(Clone, Copy, Debug)]
struct XTestFakeInputRequest {
    event_type: u8,
    detail: u8,
    root_x: i16,
    root_y: i16,
}

impl XTestFakeInputRequest {
    fn from_request(request: &crate::XWireRequest) -> Option<Self> {
        let crate::XWireRequest::XTestFakeInput {
            event_type,
            detail,
            root_x,
            root_y,
            ..
        } = *request
        else {
            return None;
        };
        Some(Self {
            event_type,
            detail,
            root_x,
            root_y,
        })
    }
}

/// What an accepted FakeInput resolves to, once the runtime has been asked
/// what it needs to know.
///
/// The target is provenance for a key and a button: the recipient is resolved
/// from grab-then-focus at execution regardless. For motion it is also the
/// surface the executor routes through, so it has to be one the registry
/// knows.
#[derive(Clone, Copy, Debug)]
enum XTestPlan {
    Key {
        target: SurfaceId,
        keycode: u32,
        pressed: bool,
    },
    Button {
        target: SurfaceId,
        button: u32,
        pressed: bool,
    },
    Motion {
        target: SurfaceId,
        global: sophia_protocol::Point,
        local: sophia_protocol::Point,
    },
}

/// Why an accepted FakeInput was not turned into a plan.
///
/// None of these is an error the client hears about. FakeInput has no reply
/// and the reference server sends none, so synthesising an error here would
/// desynchronise the client's sequence accounting. They are recorded instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum XTestUnplanned {
    /// A key or a button with focus on the root, or on a window this
    /// namespace cannot see: there is no client to receive it.
    NoTarget,
    /// A wheel button. It has no evdev button and becomes an axis step on
    /// press; a release of one is nothing at all.
    WheelRelease,
    /// The root's geometry could not be read, so nothing can be clipped
    /// against it.
    NoRoot,
}

/// How a wait on this connection ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum XTestWaitEnd {
    /// The thing waited for happened: the delay elapsed, or the outcome
    /// arrived, or the work was refused outright and there is nothing to
    /// wait for.
    Settled,
    /// The admission was revoked while waiting. The request is over; nothing
    /// it asked for will happen and nothing is owed to the client for it.
    Cancelled,
    /// The peer is gone. Its remaining requests are moot, and dispatch ends
    /// the same way an ordinary EOF ends it.
    Departed,
}

/// The evdev button an X button number names, where one exists.
///
/// X numbers its buttons from one; the executor takes evdev codes and knows
/// five. Buttons four through seven are the wheel and have no evdev button
/// at all: everywhere else they map to axis input, and they do here too.
fn xtest_evdev_button(button: u8) -> Option<u32> {
    match button {
        1 => Some(272),
        2 => Some(274),
        3 => Some(273),
        8 => Some(275),
        9 => Some(276),
        _ => None,
    }
}

/// The axis step a wheel button press is.
fn xtest_wheel_axis(button: u8) -> Option<(i32, i32)> {
    match button {
        4 => Some((0, -120)),
        5 => Some((0, 120)),
        6 => Some((-120, 0)),
        7 => Some((120, 0)),
        _ => None,
    }
}

/// The connection's injection state.
///
/// One notifier serves every wait this connection makes: the delay, the park
/// on a completion, the retry after a saturated refusal, and the gate's wake
/// on revocation. Made once, at issuance, rather than on first use. Only
/// injectors get one, and an admitted injector is a rare thing, so the
/// descriptor economy that justifies laziness elsewhere does not apply; what
/// applies instead is that a creation failure at issuance has a clean answer,
/// the client is simply not admitted to inject, where a failure mid-request
/// has none.
struct XTestConnection {
    injector: Box<dyn crate::XTestInjector>,
    notifier: crate::ConnectionNotifier,
    barrier: crate::PrivateRequestBarrier,
    /// This connection's revocation witness. `None` when the connection has
    /// no private lifecycle, in which case nothing can revoke it and every
    /// wait ends only on its own condition or on departure.
    gate: Option<PrivateLifecycleGate>,
    /// GrabControl's. Belongs to the connection and persists until the same
    /// client clears it or departs.
    #[cfg_attr(not(test), allow(dead_code))] // Read by the pause loop next.
    impervious: bool,
}

impl XTestConnection {
    /// Take up an issued injector, with everything its waits will need.
    ///
    /// The barrier is installed on the injector here, before any submission
    /// can exist, so no request can complete before there is somewhere to put
    /// its answer. The notifier is parked on the gate here too, and the
    /// register-then-reread obligation is met by every wait re-reading the
    /// gate before it sleeps.
    fn new(
        injector: Box<dyn crate::XTestInjector>,
        gate: Option<PrivateLifecycleGate>,
    ) -> std::io::Result<Self> {
        let notifier = crate::ConnectionNotifier::new()?;
        let barrier = crate::PrivateRequestBarrier::over(&notifier);
        if !injector.report_completions_to(barrier.clone()) {
            return Err(std::io::Error::other(
                "the injector already reports its completions elsewhere",
            ));
        }
        if let Some(gate) = &gate {
            gate.wake_on_close(&notifier);
        }
        Ok(Self {
            injector,
            notifier,
            barrier,
            gate,
            impervious: false,
        })
    }

    fn gate_open(&self) -> bool {
        self.gate.as_ref().is_none_or(PrivateLifecycleGate::is_open)
    }

    /// One round of waiting, shared by every wait this connection makes.
    ///
    /// No backstop timer beyond the caller's own deadline, for the reason the
    /// server-grab pause loop gives: a timer would convert a missing wake into
    /// a slow poll and hide the defect instead of failing on it. A poll
    /// failure over descriptors this server owns is a local fault rather than
    /// peer behaviour, so it keeps the unclassified constructor and reaches
    /// the reaper as the server problem it is.
    fn wait_once(
        &self,
        stream: &UnixStream,
        deadline: Option<std::time::Instant>,
    ) -> Result<crate::ConnectionWake, X11SetupSocketError> {
        crate::ConnectionWait::new(std::os::fd::AsFd::as_fd(stream), &self.notifier)
            .wait_until(deadline)
            .map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to wait for synthetic input to settle: {error}"
                ))
            })
    }

    /// Wait out a FakeInput's delay, on bounded monotonic state.
    ///
    /// The domain is the full CARD32, so the sum can exceed what an instant
    /// will represent. Overflow becomes a wait with no deadline, which is the
    /// honest reading of a delay longer than this process's clock can name:
    /// it does not expire on its own, and revocation and departure are what
    /// end it. Taken before the request is validated, as the reference does,
    /// so a malformed request carrying a delay waits and only then answers
    /// its error.
    fn delay(
        &self,
        stream: &UnixStream,
        milliseconds: u32,
    ) -> Result<XTestWaitEnd, X11SetupSocketError> {
        let deadline = std::time::Instant::now()
            .checked_add(std::time::Duration::from_millis(u64::from(milliseconds)));
        loop {
            if !self.gate_open() {
                return Ok(XTestWaitEnd::Cancelled);
            }
            match self.wait_once(stream, deadline)? {
                crate::ConnectionWake::Deadline => return Ok(XTestWaitEnd::Settled),
                // A stale wake from an earlier request, or a revocation; the
                // gate is re-read at the top either way.
                crate::ConnectionWake::Notified => continue,
                crate::ConnectionWake::Departed => return Ok(XTestWaitEnd::Departed),
            }
        }
    }

    /// Park until the outstanding request's internal processing completes.
    ///
    /// The outcome is checked before every sleep. The eventfd counts, so a
    /// wake raised before this wait began is still pending and is never slept
    /// through; what the recheck catches is a value stored while the wake
    /// itself was lost. `half_closed` is deliberately never consulted: bytes
    /// the peer already sent remain buffered and it is still entitled to have
    /// them processed, so a write-half-close finishes this wait and then the
    /// ordinary read drains what follows until the socket genuinely ends.
    fn await_processing(&self, stream: &UnixStream) -> Result<XTestWaitEnd, X11SetupSocketError> {
        loop {
            if self.barrier.take().is_some() {
                return Ok(XTestWaitEnd::Settled);
            }
            if !self.gate_open() {
                return Ok(XTestWaitEnd::Cancelled);
            }
            match self.wait_once(stream, None)? {
                crate::ConnectionWake::Notified | crate::ConnectionWake::Deadline => continue,
                crate::ConnectionWake::Departed => return Ok(XTestWaitEnd::Departed),
            }
        }
    }

    /// Resolve an accepted FakeInput against what the runtime knows, under a
    /// guard taken for this alone and released before anything is submitted.
    ///
    /// The target for every kind is the focused window's surface, and the
    /// focus is the namespace's, not this connection's. Focus is seat-wide in
    /// X: the observer that will receive the event set it, from its own
    /// connection, and the injector never did. A per-connection projection
    /// of focus therefore still points at the root here, which has no surface
    /// and would resolve nothing. The recipient is resolved from
    /// grab-then-focus at execution against that same seat-wide focus.
    fn plan(
        &self,
        runtime: &XAuthorityRuntime,
        namespace: NamespaceId,
        request: XTestFakeInputRequest,
    ) -> Result<XTestPlan, XTestUnplanned> {
        let (focused_window, _revert_to) = runtime.input_focus(namespace);
        let focused_surface = runtime.window_surface(namespace, focused_window);
        // A key or a button needs a client to receive it, so focus on the
        // root, which is no client's window, leaves them nowhere to go. A
        // motion is different: the pointer can be over the bare root, and
        // the executor names that state with a surface of its own so the
        // position can move and a client that selected motion on the root
        // can be told.
        let target = match (focused_surface, request.event_type) {
            (Some(surface), _) => surface,
            (None, 6) => crate::ROOT_POINTER_SURFACE,
            (None, _) => return Err(XTestUnplanned::NoTarget),
        };
        match request.event_type {
            2 | 3 => Ok(XTestPlan::Key {
                target,
                // The executor takes evdev; X keycodes sit eight above it.
                // The dispatcher refused anything below eight.
                keycode: u32::from(request.detail) - 8,
                pressed: request.event_type == 2,
            }),
            4 | 5 => {
                let pressed = request.event_type == 4;
                if let Some(button) = xtest_evdev_button(request.detail) {
                    return Ok(XTestPlan::Button {
                        target,
                        button,
                        pressed,
                    });
                }
                // A wheel button: an axis step on press, nothing on release.
                // Carried as motion of no distance with the axis attached is
                // how the executor spells it, but the injector trait has no
                // axis method yet, so the press is dropped and recorded for
                // now rather than misreported as a button.
                let _ = xtest_wheel_axis(request.detail);
                Err(XTestUnplanned::WheelRelease)
            }
            _ => {
                let root = runtime
                    .drawable_facts(
                        namespace,
                        crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1),
                    )
                    .map_err(|_| XTestUnplanned::NoRoot)?
                    .geometry;
                let current = runtime
                    .input_authority_mut()
                    .pointer_query_state(namespace)
                    .position
                    .map_or((0, 0), |pointer| {
                        (i32::from(pointer.root_x), i32::from(pointer.root_y))
                    });
                let (x, y) = if request.detail == crate::X_TEST_MOTION_ABSOLUTE {
                    (i32::from(request.root_x), i32::from(request.root_y))
                } else {
                    (
                        current.0.saturating_add(i32::from(request.root_x)),
                        current.1.saturating_add(i32::from(request.root_y)),
                    )
                };
                // Never refused, only clipped, and the highest reachable is
                // one less than the width and the height. The root named in
                // the request adds its own origin, which for the one screen
                // this instance has is zero.
                let x = x.clamp(0, root.width.saturating_sub(1).max(0));
                let y = y.clamp(0, root.height.saturating_sub(1).max(0));
                let (origin_x, origin_y) = runtime
                    .window_root_position(focused_window)
                    .unwrap_or((0, 0));
                Ok(XTestPlan::Motion {
                    target,
                    global: sophia_protocol::Point {
                        x: f64::from(x),
                        y: f64::from(y),
                    },
                    local: sophia_protocol::Point {
                        x: f64::from(x.saturating_sub(origin_x)),
                        y: f64::from(y.saturating_sub(origin_y)),
                    },
                })
            }
        }
    }

    fn submit(&self, plan: XTestPlan) -> Result<crate::XTestAccepted, crate::XTestInjectionRefusal> {
        match plan {
            XTestPlan::Key {
                target,
                keycode,
                pressed,
            } => self.injector.submit_key(target, keycode, pressed),
            XTestPlan::Button {
                target,
                button,
                pressed,
            } => self.injector.submit_button(target, button, pressed),
            XTestPlan::Motion {
                target,
                global,
                local,
            } => self.injector.submit_motion(target, global, local),
        }
    }

    /// Hand a plan to the authority and wait until it has been processed.
    ///
    /// This is FakeInput's whole obligation to the next request: not until
    /// the work was accepted, and not until it reached a socket, but until
    /// its internal processing completed. A refusal on the terms of the
    /// request ends it with nothing owed; the client hears nothing, because
    /// FakeInput has no reply and the reference sends none.
    ///
    /// Saturated is the one refusal that is not an answer. It means the
    /// grant's one completion cell is still held by the request before this
    /// one, and it is worth retrying exactly when that request is observed,
    /// because observing is what frees the cell and is also what raises this
    /// connection's wake. So the retry parks on the same notifier and asks
    /// again, with no attempt limit: a wedged runner is bounded by the
    /// execution watchdog, which ends the transport and arrives here as
    /// departure.
    fn submit_and_await(
        &self,
        stream: &UnixStream,
        plan: XTestPlan,
    ) -> Result<XTestWaitEnd, X11SetupSocketError> {
        loop {
            match self.submit(plan) {
                Ok(_accepted) => return self.await_processing(stream),
                Err(crate::XTestInjectionRefusal::Saturated) => {
                    if !self.gate_open() {
                        return Ok(XTestWaitEnd::Cancelled);
                    }
                    match self.wait_once(stream, None)? {
                        crate::ConnectionWake::Notified | crate::ConnectionWake::Deadline => {
                            continue;
                        }
                        crate::ConnectionWake::Departed => return Ok(XTestWaitEnd::Departed),
                    }
                }
                Err(_refused) => return Ok(XTestWaitEnd::Settled),
            }
        }
    }
}
