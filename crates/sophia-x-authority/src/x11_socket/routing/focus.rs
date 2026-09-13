/// One effect a control operation queued on another client's writer, which
/// reports its own end.
///
/// Held by the queued entry itself, so the report happens whether the entry is
/// run or given up unrun: a queue that goes takes its entries with it, and a
/// dependent effect that vanished silently would leave its origin waiting on
/// something that can no longer happen.
///
/// Not a receipt. Which of the two ends it was is not recorded and is not an
/// outcome: what it establishes is only that this particular effect can no
/// longer happen.
#[cfg(unix)]
struct X11DependentEffect {
    registry: ControlCompletionRegistry,
    origin: ControlCompletionToken,
}

/// Names the origin and nothing else.
///
/// Written rather than derived so that a routed control's debug output cannot
/// grow to include what any client asked for: the registry behind this holds
/// every accepted command.
#[cfg(unix)]
impl core::fmt::Debug for X11DependentEffect {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("X11DependentEffect")
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}

#[cfg(unix)]
impl X11DependentEffect {
    /// Queue one against its origin, if the origin still has a record.
    fn note(
        registry: &ControlCompletionRegistry,
        origin: ControlCompletionToken,
    ) -> Option<Self> {
        registry.note_dependent(origin).then(|| Self {
            registry: registry.clone(),
            origin,
        })
    }
}

#[cfg(unix)]
impl Drop for X11DependentEffect {
    fn drop(&mut self) {
        self.registry.dependent_ended(self.origin);
    }
}

#[cfg(unix)]
#[derive(Debug)]
enum X11RoutedControl {
    Authority {
        command: XAuthorityControlCommand,
        focus: Option<X11FocusTransition>,
        /// The private path's completion registration, when there is one.
        ///
        /// Travels with the command because the writer is where its outcome
        /// becomes known, and the writer sees only a client and a public
        /// transaction otherwise -- which alias across requests.
        completion: Option<ControlCompletionToken>,
    },
    FocusOut {
        window: XResourceId,
        time_msec: u32,
        /// The operation whose routing queued this, when one did.
        ///
        /// Carried so that this effect ending is reported against the
        /// operation that caused it. Nothing linked the two before, so an
        /// operation's own router and writer could both go quiet while this
        /// still sat in another connection's queue.
        origin: Option<X11DependentEffect>,
    },
}

#[cfg(all(unix, test))]
impl X11RoutedControl {
    fn authority_command(&self) -> Option<XAuthorityControlCommand> {
        match self {
            Self::Authority { command, .. } => Some(*command),
            Self::FocusOut { .. } => None,
        }
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum X11FocusTransition {
    Unchanged,
    Enter {
        previous: Option<XResourceId>,
        time_msec: u32,
    },
    Clear {
        previous: Option<XResourceId>,
        time_msec: u32,
    },
}

#[cfg(unix)]
fn x11_server_time_msec() -> u32 {
    let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let seconds = u64::try_from(now.tv_sec).unwrap_or_default();
    let nanos = u64::try_from(now.tv_nsec).unwrap_or_default();
    let milliseconds = seconds
        .saturating_mul(1_000)
        .saturating_add(nanos / 1_000_000);
    let bytes = milliseconds.to_le_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[cfg(unix)]
impl XServerFrontendRouteRegistry {
    fn route_focus_control(
        &self,
        route: XAuthorityClientControlCommand,
        completion: Option<ControlCompletionToken>,
    ) -> Option<Result<(), XServerFrontendRouteError>> {
        match route.command {
            XAuthorityControlCommand::FocusSurface { surface, .. } => {
                Some(self.route_focus_surface(route, surface, completion))
            }
            XAuthorityControlCommand::ClearFocus { .. } => Some(self.route_clear_focus(route, completion)),
            _ => None,
        }
    }

    fn route_focus_surface(
        &self,
        route: XAuthorityClientControlCommand,
        surface: SurfaceId,
        completion: Option<ControlCompletionToken>,
    ) -> Result<(), XServerFrontendRouteError> {
        let target = self
            .surfaces
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .get(&surface)
            .copied();
        let Some(target) = target else {
            return self.route_authority_control(route, None, completion);
        };
        if target.client != route.client {
            return Err(XServerFrontendRouteError::UnknownClient {
                client: route.client,
            });
        }
        let mut focused = self
            .focused_surface
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let time_msec = x11_server_time_msec();
        let transition = match *focused {
            Some(previous) if previous == target => X11FocusTransition::Unchanged,
            Some(previous) if previous.client == target.client => X11FocusTransition::Enter {
                previous: Some(previous.window),
                time_msec,
            },
            Some(previous) => {
                self.route_focus_out(previous, time_msec, completion)?;
                X11FocusTransition::Enter {
                    previous: None,
                    time_msec,
                }
            }
            None => X11FocusTransition::Enter {
                previous: None,
                time_msec,
            },
        };
        self.route_authority_control(route, Some(transition), completion)?;
        *focused = Some(target);
        Ok(())
    }

    fn route_clear_focus(
        &self,
        route: XAuthorityClientControlCommand,
        completion: Option<ControlCompletionToken>,
    ) -> Result<(), XServerFrontendRouteError> {
        let mut focused = self
            .focused_surface
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let time_msec = x11_server_time_msec();
        let transition = match *focused {
            Some(previous) if previous.client == route.client => X11FocusTransition::Clear {
                previous: Some(previous.window),
                time_msec,
            },
            Some(previous) => {
                self.route_focus_out(previous, time_msec, completion)?;
                X11FocusTransition::Clear {
                    previous: None,
                    time_msec,
                }
            }
            None => X11FocusTransition::Clear {
                previous: None,
                time_msec,
            },
        };
        self.route_authority_control(route, Some(transition), completion)?;
        *focused = None;
        Ok(())
    }

    fn route_focus_out(
        &self,
        previous: XServerFrontendSurfaceRoute,
        time_msec: u32,
        origin: Option<ControlCompletionToken>,
    ) -> Result<(), XServerFrontendRouteError> {
        let sender = self.client_senders(previous.client)?.control;
        // Counted against its origin before it is queued, so there is no
        // moment where the effect exists and nothing is waiting for it.
        let origin = origin.zip(self.control_completion.get()).and_then(
            |(origin, registry)| X11DependentEffect::note(registry, origin),
        );
        self.route_to_client(
            previous.client,
            sender,
            X11RoutedControl::FocusOut {
                window: previous.window,
                time_msec,
                origin,
            },
        )
    }

    fn route_authority_control(
        &self,
        route: XAuthorityClientControlCommand,
        focus: Option<X11FocusTransition>,
        completion: Option<ControlCompletionToken>,
    ) -> Result<(), XServerFrontendRouteError> {
        let sender = self.client_senders(route.client)?.control;
        self.route_to_client(
            route.client,
            sender,
            X11RoutedControl::Authority {
                command: route.command,
                focus,
                completion,
            },
        )
    }
}
