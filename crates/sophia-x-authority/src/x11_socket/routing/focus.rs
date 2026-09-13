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
        origin: Option<ControlDependent>,
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
        //
        // A governed request whose dependency cannot be counted is refused
        // rather than queued untracked. Falling through to `None` there made
        // the work look ungoverned, which is a real state for ordinary work
        // and a false one for this: the operation would have been settled
        // while an effect of it was still sitting in another queue.
        let origin = match (origin, self.control_completion.get()) {
            (None, _) => None,
            (Some(origin), Some(registry)) => Some(
                registry
                    .track_dependent(origin)
                    .map_err(|refusal| XServerFrontendRouteError::DependentNotTracked {
                        client: previous.client,
                        refusal,
                    })?,
            ),
            (Some(_), None) => {
                return Err(XServerFrontendRouteError::DependentNotTracked {
                    client: previous.client,
                    refusal: ControlDependentRefusal::Unavailable,
                });
            }
        };
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
