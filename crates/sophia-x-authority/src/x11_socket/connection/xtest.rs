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
    /// Focus is on the root, or on a window this namespace cannot see, so
    /// there is no surface to name. Until root-targeted motion lands this is
    /// where injection at the bare root stops.
    NoTarget,
    /// A wheel button. It has no evdev button and becomes an axis step on
    /// press; a release of one is nothing at all.
    WheelRelease,
    /// The root's geometry could not be read, so nothing can be clipped
    /// against it.
    NoRoot,
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
struct XTestConnection {
    injector: Box<dyn crate::XTestInjector>,
    /// GrabControl's. Belongs to the connection and persists until the same
    /// client clears it or departs.
    #[cfg_attr(not(test), allow(dead_code))] // Read by the pause loop next.
    impervious: bool,
}

impl XTestConnection {
    fn new(injector: Box<dyn crate::XTestInjector>) -> Self {
        Self {
            injector,
            impervious: false,
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
        let target = runtime
            .window_surface(namespace, focused_window)
            .ok_or(XTestUnplanned::NoTarget)?;
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

    /// Hand a plan to the authority. Acceptance and not completion: what
    /// comes back says the work was ordered.
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
}
