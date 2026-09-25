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

    /// The motion an accepted WarpPointer resolves to: absolute, at the
    /// position the dispatcher placed.
    fn absolute_motion(root_x: i16, root_y: i16) -> Self {
        Self {
            event_type: 6,
            detail: crate::X_TEST_MOTION_ABSOLUTE,
            root_x,
            root_y,
        }
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
        /// Where the pointer is. A button carries no position of its own.
        global: sophia_protocol::Point,
        local: sophia_protocol::Point,
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

/// A root position and the same position relative to the focused window,
/// which is the surface a motion or button is submitted against. Motion and
/// buttons share this so a button can never again be placed somewhere motion
/// would not have put it.
fn pointer_points(
    runtime: &XAuthorityRuntime,
    focused_window: crate::XResourceId,
    x: i32,
    y: i32,
) -> (sophia_protocol::Point, sophia_protocol::Point) {
    let (origin_x, origin_y) = runtime
        .window_root_position(focused_window)
        .unwrap_or((0, 0));
    (
        sophia_protocol::Point {
            x: f64::from(x),
            y: f64::from(y),
        },
        sophia_protocol::Point {
            x: f64::from(x.saturating_sub(origin_x)),
            y: f64::from(y.saturating_sub(origin_y)),
        },
    )
}

/// Where a pointer event goes: the toplevel under the point when this
/// instance is the one that placed the toplevels (the conformance host, where
/// no Engine stacks them and a suite's window is a plain toplevel), else the
/// focused surface the plan already resolved, which is the Engine's business
/// to have put under the pointer. Returns the target surface and the window
/// the local position is measured from.
fn pointer_target(
    runtime: &XAuthorityRuntime,
    namespace: NamespaceId,
    focused_window: crate::XResourceId,
    focused_target: Option<SurfaceId>,
    x: i32,
    y: i32,
) -> Option<(SurfaceId, crate::XResourceId)> {
    runtime
        .client_placed_toplevel_at(namespace, x, y)
        .and_then(|toplevel| runtime.window_surface(namespace, toplevel).map(|surface| (surface, toplevel)))
        .or_else(|| focused_target.map(|target| (target, focused_window)))
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
    /// This connection's own input writer's progress, so an injection is
    /// not followed by the next request's reply before what it owed this
    /// client has reached the socket. `None` where the connection has no
    /// routed writer, in which case nothing of the kind can be queued.
    watermark: Option<Arc<X11InputWatermark>>,
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
        watermark: Option<Arc<X11InputWatermark>>,
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
        if let Some(watermark) = &watermark {
            watermark.wake_on_drain(&notifier);
        }
        Ok(Self {
            injector,
            notifier,
            barrier,
            watermark,
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

    /// Park until this connection's own input writer has finished with
    /// everything routed to it so far.
    ///
    /// Processing ends at routing: the event is queued for the writers, and
    /// this connection's next request would otherwise be read, and its
    /// reply written, while the event the injection owed this same client
    /// was still in its queue (t229). The mark is taken after processing,
    /// so it covers what this injection queued. Bounded, because the writer
    /// may be parked on the keyboard readiness wait, which is up to five
    /// seconds long and not this request's to serve out; the bound is far
    /// beyond the microseconds an ordinary write takes.
    fn await_drain(&self, stream: &UnixStream) -> Result<XTestWaitEnd, X11SetupSocketError> {
        let Some(watermark) = self.watermark.as_deref() else {
            return Ok(XTestWaitEnd::Settled);
        };
        let mark = watermark.mark();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            if watermark.reached(mark) {
                return Ok(XTestWaitEnd::Settled);
            }
            if !self.gate_open() {
                return Ok(XTestWaitEnd::Cancelled);
            }
            match self.wait_once(stream, Some(deadline))? {
                crate::ConnectionWake::Deadline => return Ok(XTestWaitEnd::Settled),
                crate::ConnectionWake::Notified => continue,
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
        let focused_target = match (focused_surface, request.event_type) {
            (Some(surface), _) => Some(surface),
            (None, 6) => Some(crate::ROOT_POINTER_SURFACE),
            (None, _) => None,
        };
        match request.event_type {
            2 | 3 => Ok(XTestPlan::Key {
                // With the focus on the root the protocol's PointerRoot rule
                // applies: the key goes to the window under the pointer,
                // which this instance resolves only where it placed the
                // toplevels itself.
                target: focused_target
                    .or_else(|| {
                        let (x, y) = runtime
                            .input_authority_mut()
                            .pointer_query_state(namespace)
                            .position
                            .map_or((0, 0), |pointer| {
                                (i32::from(pointer.root_x), i32::from(pointer.root_y))
                            });
                        runtime
                            .client_placed_toplevel_at(namespace, x, y)
                            .and_then(|toplevel| runtime.window_surface(namespace, toplevel))
                    })
                    .ok_or(XTestUnplanned::NoTarget)?,
                // The executor takes evdev; X keycodes sit eight above it.
                // The dispatcher refused anything below eight.
                keycode: u32::from(request.detail) - 8,
                pressed: request.event_type == 2,
            }),
            4 | 5 => {
                let pressed = request.event_type == 4;
                if let Some(button) = xtest_evdev_button(request.detail) {
                    // The request's own root and coordinates are ignored for a
                    // button, as the reference ignores them: it happens where
                    // the pointer already is. Submitting no position put
                    // every XTEST press and release at the screen origin, and
                    // a drag into xterm became a zero-length selection (t155).
                    let (x, y) = runtime
                        .input_authority_mut()
                        .pointer_query_state(namespace)
                        .position
                        .map_or((0, 0), |pointer| {
                            (i32::from(pointer.root_x), i32::from(pointer.root_y))
                        });
                    let (target, anchor) =
                        pointer_target(runtime, namespace, focused_window, focused_target, x, y)
                            .ok_or(XTestUnplanned::NoTarget)?;
                    let (global, local) = pointer_points(runtime, anchor, x, y);
                    return Ok(XTestPlan::Button {
                        target,
                        button,
                        pressed,
                        global,
                        local,
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
                let (target, anchor) =
                    pointer_target(runtime, namespace, focused_window, focused_target, x, y)
                        .ok_or(XTestUnplanned::NoTarget)?;
                let (global, local) = pointer_points(runtime, anchor, x, y);
                Ok(XTestPlan::Motion {
                    target,
                    global,
                    local,
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
                global,
                local,
            } => self.injector.submit_button(target, button, pressed, global, local),
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
                Ok(_accepted) => {
                    return match self.await_processing(stream)? {
                        XTestWaitEnd::Settled => self.await_drain(stream),
                        end => Ok(end),
                    };
                }
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
