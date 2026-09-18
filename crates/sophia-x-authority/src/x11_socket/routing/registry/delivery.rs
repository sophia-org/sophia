/// The exact connection a protocol delivery could not be queued for.
///
/// MINTED ONLY BY THE ROUTE THAT FAILED, from the identity it captured with
/// the endpoint it used. A caller holding one can end THAT connection; it
/// cannot end whoever holds the number by the time it acts. It carries no
/// other capability and settles nothing.
#[cfg(unix)]
pub(crate) struct XServerFrontendStalledRecipient {
    client: XServerFrontendClientId,
    occupant: Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
}

#[cfg(unix)]
impl XServerFrontendStalledRecipient {
    pub(crate) fn client(&self) -> XServerFrontendClientId {
        self.client
    }
}

/// Why a delivery to a watcher did not happen.
#[cfg(unix)]
pub(crate) enum XServerFrontendWatcherRefusal {
    /// It stopped draining. Ending it goes through this value, not the
    /// number.
    Stalled(XServerFrontendStalledRecipient),
    /// Anything else the route reported.
    Route(XServerFrontendRouteError),
}

impl XServerFrontendRouteRegistry {
    fn advance_input_control_epoch(&self) -> Result<usize, XServerFrontendRouteError> {
        self.input_authority
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .advance_security_epoch();
        self.pointer_state.lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?.clear();
        let drained = {
            let mut frozen = self
                .frozen_input
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
            std::mem::take(&mut *frozen)
        };
        let count = drained.len();
        for deferred in drained {
            self.send_input_delivery(
                deferred.client,
                deferred.route.delivery,
                XAuthorityInputDeliveryOutcome::EpochRevoked,
            )?;
        }
        Ok(count)
    }

    /// Clear everything an X security epoch revokes, in ranked order.
    ///
    /// This is the privileged control path, not input execution. A transition
    /// cancels reservations and revokes grants, so running it through the
    /// execution transaction would demand the very authority the transition is
    /// in the middle of withdrawing, and the cleanup that has to outlive a
    /// grant would deadlock against its own revocation. The caller holds the
    /// common guard; this takes the later-ranked X guards beneath it.
    ///
    /// The order is pointer state, then frozen input, then the input
    /// authority. That is not arbitrary: client teardown already co-holds
    /// pointer state and then the input authority, so taking them the other
    /// way round here would be a reverse nesting against a live caller.
    ///
    /// The three are held together rather than taken and dropped one at a
    /// time, so no observer sees grabs cleared while pointer state still
    /// describes the revision being replaced.
    ///
    /// Nothing is delivered from in here. The frozen queue is handed back so
    /// its receipts can be sent once every guard is released, because a
    /// transition must not wait on anything while it holds them.
    fn clear_revoked_x_populations(
        &self,
    ) -> Result<VecDeque<XDeferredRoutedInput>, XServerFrontendRouteError> {
        let mut pointer_state = self
            .pointer_state
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let mut frozen_input = self
            .frozen_input
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let mut input_authority = self
            .input_authority
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;

        pointer_state.clear();
        let drained = std::mem::take(&mut *frozen_input);
        input_authority.advance_security_epoch();

        Ok(drained)
    }

    /// Send the receipts a cleared transition owes, after its guards are gone.
    fn report_revoked_input(
        &self,
        drained: VecDeque<XDeferredRoutedInput>,
    ) -> Result<usize, XServerFrontendRouteError> {
        let count = drained.len();
        for deferred in drained {
            self.send_input_delivery(
                deferred.client,
                deferred.route.delivery,
                XAuthorityInputDeliveryOutcome::EpochRevoked,
            )?;
        }
        Ok(count)
    }

    fn release_route_lease(
        &self,
        release: XAuthorityRouteLeaseRelease,
    ) -> Result<(), XServerFrontendRouteError> {
        let client = self
            .clients
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .iter()
            .find_map(|(client, route)| {
                (route.admission == Some(release.admission)).then_some(*client)
            });
        if let Some(client) = client {
            self.input_authority
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                .ungrab_pointer(release.admission.namespace.id, client.raw());
        }
        let Some(sender) = self.route_lease_update_sender.as_ref() else {
            return Ok(());
        };
        let _ = sender.send(XAuthorityRouteLeaseUpdate {
            identity: release.identity,
            target_surface: self
                .surfaces
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                .iter()
                .find_map(|(surface, route)| {
                    (route.admission == Some(release.admission)).then_some(*surface)
                })
                .unwrap_or(SurfaceId::INVALID),
            admission: release.admission,
            kind: XAuthorityRouteLeaseUpdateKind::Released,
        });
        Ok(())
    }

    /// Route an event whose admission has already been decided.
    ///
    /// The decision is a parameter because comparing two epochs is only how it
    /// is reached without a coordinator. With one, admission also depends on
    /// the publication and on whether a transition is in flight, none of which
    /// a single equality can express.
    fn route_engine_input_admitted(
        &self,
        route: XAuthorityRoutedInput,
        stamp: crate::ControlStamp,
        admitted: bool,
    ) -> Result<(), XServerFrontendRouteError> {
        if !self.input_recovery.begin_routing(route.delivery) { return Ok(()); }
        // An event stamped with a closed epoch is one the session revoked
        // between routing and delivery, not one that failed to route.
        if !admitted {
            // Preserve the known target owner in the receipt without binding
            // it as the receiving client: grab routing never happened.
            let client = self.surfaces.lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                .get(&route.request.target_surface).map_or(XServerFrontendClientId(0), |route| route.client);
            self.send_input_delivery(client, route.delivery,
                XAuthorityInputDeliveryOutcome::EpochRevoked)?;
            return Ok(());
        }
        if route.mode == XAuthorityRoutedInputMode::StateOnly {
            if let InputEventKind::Key { keycode, pressed } = route.request.kind {
                let mapped = self.xkb_worker.request(XkbWorkerCommand::Key {
                    seat: route.request.seat,
                    keycode,
                    pressed,
                })?;
                let surface = self.surfaces.lock()
                    .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                    .get(&route.request.target_surface).copied();
                if let (Some(surface), Some((_, _, modifiers))) = (surface, mapped) {
                    let mut authority = self.input_authority.lock()
                        .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
                    if !authority.keyboard_frozen(surface.namespace) {
                        authority.observe_query_modifiers(surface.namespace, modifiers);
                    }
                }
            }
            return Ok(());
        }
        let surface_route = self
            .surfaces
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .get(&route.request.target_surface)
            .copied()
            ;
        let Some(surface_route) = surface_route else {
            return self.send_input_delivery(XServerFrontendClientId(0), route.delivery,
                XAuthorityInputDeliveryOutcome::TargetGone);
        };
        if self.route_is_frozen(&route, surface_route.namespace)? {
            let mut frozen = self
                .frozen_input
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
            if frozen.len() >= self.per_client_input_capacity.get() {
                drop(frozen);
                tracing::warn!("sophia_x11_input_route status=rejected reason=frozen_queue_full client={} content=redacted", surface_route.client.raw());
                self.send_input_delivery(
                    surface_route.client,
                    route.delivery,
                    XAuthorityInputDeliveryOutcome::RouteRejected,
                )?;
                return Err(XServerFrontendRouteError::ClientQueueFull {
                    client: surface_route.client,
                });
            }
            frozen.push_back(XDeferredRoutedInput {
                publication: stamp.publication,
                client: surface_route.client,
                control_epoch: stamp.control_epoch,
                route,
            });
            return Ok(());
        }
        let mut client = surface_route.client;
        let mut button_lease_update = None;
        // Engine already selected the committed target surface. Preserve its
        // owning window as the start of core propagation; X grabs may replace
        // it below, but event-mask update order must never choose the target.
        let mut target_window = Some(surface_route.window);
        let mut pointers = self
            .pointer_state
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let pointer = pointers
            .entry((surface_route.namespace, route.request.seat))
            .or_insert_with(crate::XCorePointerMapper::new);
        let time_msec = u32::try_from(route.request.time_msec).unwrap_or(u32::MAX);
        let event = match route.request.kind {
            InputEventKind::Key { .. } => {
                let XKeyboardRouteResolution::Routed(resolved) = resolve_x_keyboard_input(
                    &route,
                    surface_route,
                    &self.input_authority,
                    &self.xkb_worker,
                    pointer.state(),
                    time_msec,
                )?
                else {
                    tracing::warn!("sophia_x11_input_route status=rejected reason=keyboard_mapping client={} content=redacted", client.raw());
                    return self.send_input_delivery(
                        client,
                        route.delivery,
                        XAuthorityInputDeliveryOutcome::RouteRejected,
                    );
                };
                client = resolved.client;
                target_window = resolved.target_window;
                resolved.event
            }
            InputEventKind::PointerMotion => {
                if let Some(grab) = self
                    .input_authority
                    .lock()
                    .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                    .pointer_grab(surface_route.namespace)
                {
                    client = XServerFrontendClientId(grab.owner);
                    target_window = Some(if grab.owner_events && client == surface_route.client {
                        surface_route.window
                    } else {
                        grab.window
                    });
                }
                XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                    kind: XAuthorityPointerEventKind::Motion,
                    surface: route.request.target_surface,
                    root_x: clamp_input_coordinate(route.request.global_position.x),
                    root_y: clamp_input_coordinate(route.request.global_position.y),
                    event_x: clamp_input_coordinate(route.request.local_position.x),
                    event_y: clamp_input_coordinate(route.request.local_position.y),
                    state: self
                        .xkb_worker
                        .request(XkbWorkerCommand::Modifiers {
                            seat: route.request.seat,
                        })?
                        .map_or(0, |(_, state, _)| state)
                        | pointer.state(),
                    time_msec,
                })
            }
            InputEventKind::PointerButton { button, pressed } => {
                if let Some(grab) = self
                    .input_authority
                    .lock()
                    .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                    .pointer_grab(surface_route.namespace)
                {
                    client = XServerFrontendClientId(grab.owner);
                    target_window = Some(if grab.owner_events && client == surface_route.client {
                        surface_route.window
                    } else {
                        grab.window
                    });
                }
                let Some((button, state)) = pointer.map_evdev_button(button, pressed) else {
                    tracing::warn!("sophia_x11_input_route status=rejected reason=button_mapping client={} content=redacted", client.raw());
                    return self.send_input_delivery(
                        client,
                        route.delivery,
                        XAuthorityInputDeliveryOutcome::RouteRejected,
                    );
                };
                if pressed {
                    let mut authority = self
                        .input_authority
                        .lock()
                        .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
                    if authority.pointer_grab(surface_route.namespace).is_none() {
                        button_lease_update = Some(XAuthorityRouteLeaseUpdateKind::Confirmed);
                    }
                    let grab = authority.activate_button(
                        surface_route.namespace,
                        button,
                        state & 0xff,
                        crate::XActiveInputGrab {
                            owner: surface_route.client.raw(),
                            window: surface_route.window,
                            owner_events: true,
                            pointer_mode: 1,
                            keyboard_mode: 1,
                            event_mask: u16::MAX,
                            xi_event_mask: [0; 8],
                            xi_event_mask_words: 0,
                            route_lease: route.route_lease,
                        },
                    );
                    client = XServerFrontendClientId(grab.owner);
                    target_window = Some(if grab.owner_events && client == surface_route.client {
                        surface_route.window
                    } else {
                        grab.window
                    });
                } else {
                    let mut authority = self
                        .input_authority
                        .lock()
                        .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
                    authority.release_button(surface_route.namespace, button, pointer.state() == 0);
                    if authority.pointer_grab(surface_route.namespace).is_none() {
                        button_lease_update = Some(XAuthorityRouteLeaseUpdateKind::Released);
                    }
                }
                XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                    kind: XAuthorityPointerEventKind::Button { button, pressed },
                    surface: route.request.target_surface,
                    root_x: clamp_input_coordinate(route.request.global_position.x),
                    root_y: clamp_input_coordinate(route.request.global_position.y),
                    event_x: clamp_input_coordinate(route.request.local_position.x),
                    event_y: clamp_input_coordinate(route.request.local_position.y),
                    state: self
                        .xkb_worker
                        .request(XkbWorkerCommand::Modifiers {
                            seat: route.request.seat,
                        })?
                        .map_or(0, |(_, state, _)| state)
                        | state,
                    time_msec,
                })
            }
            InputEventKind::PointerAxis {
                horizontal_v120,
                vertical_v120,
            } => {
                if let Some(grab) = self
                    .input_authority
                    .lock()
                    .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                    .pointer_grab(surface_route.namespace)
                {
                    client = XServerFrontendClientId(grab.owner);
                    target_window = Some(if grab.owner_events && client == surface_route.client {
                        surface_route.window
                    } else {
                        grab.window
                    });
                }
                let Some(axis) = pointer.map_axis(horizontal_v120, vertical_v120) else {
                    tracing::warn!("sophia_x11_input_route status=rejected reason=axis_mapping client={} content=redacted", client.raw());
                    return self.send_input_delivery(
                        client,
                        route.delivery,
                        XAuthorityInputDeliveryOutcome::RouteRejected,
                    );
                };
                let state = self
                    .xkb_worker
                    .request(XkbWorkerCommand::Modifiers {
                        seat: route.request.seat,
                    })?
                    .map_or(0, |(_, state, _)| state)
                    | pointer.state();
                let release_state = state | pointer.axis_release_state(axis.button);
                let pointer_event = |pressed| {
                    XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                        kind: XAuthorityPointerEventKind::Axis {
                            button: axis.button,
                            pressed,
                            horizontal_position_v120: pressed
                                .then_some(axis.horizontal_position_v120)
                                .flatten(),
                            vertical_position_v120: pressed
                                .then_some(axis.vertical_position_v120)
                                .flatten(),
                        },
                        surface: route.request.target_surface,
                        root_x: clamp_input_coordinate(route.request.global_position.x),
                        root_y: clamp_input_coordinate(route.request.global_position.y),
                        event_x: clamp_input_coordinate(route.request.local_position.x),
                        event_y: clamp_input_coordinate(route.request.local_position.y),
                        state: if pressed { state } else { release_state },
                        time_msec,
                    })
                };
                drop(pointers);
                if let Err(error) = self.route_resolved_input(
                    surface_route.namespace,
                    client,
                    surface_route.window,
                    target_window,
                    pointer_event(true),
                    None,
                ) {
                    self.send_input_delivery(
                        client,
                        route.delivery,
                        XAuthorityInputDeliveryOutcome::RouteRejected,
                    )?;
                    return Err(error);
                }
                return self.route_resolved_input(
                    surface_route.namespace,
                    client,
                    surface_route.window,
                    target_window,
                    pointer_event(false),
                    route.delivery,
                );
            }
        };
        drop(pointers);
        let lease_update = route.route_lease.and_then(|identity| {
            let kind = button_lease_update?;
            let admission = self.client_senders(client).ok()?.admission?;
            Some((identity, kind, admission))
        });
        let result = self.route_resolved_input(
            surface_route.namespace,
            client,
            surface_route.window,
            target_window,
            event,
            route.delivery,
        );
        if let Some((identity, kind, admission)) = lease_update {
            let reported_kind = if result.is_ok() {
                kind
            } else {
                if kind == XAuthorityRouteLeaseUpdateKind::Confirmed {
                    self.input_authority
                        .lock()
                        .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                        .ungrab_pointer(surface_route.namespace, client.raw());
                }
                XAuthorityRouteLeaseUpdateKind::Rejected
            };
            self.send_route_lease_update(
                identity,
                route.request.target_surface,
                admission,
                reported_kind,
            )?;
        }
        result
    }

    fn route_control(
        &self,
        route: XAuthorityClientControlCommand,
    ) -> Result<(), XServerFrontendRouteError> {
        self.route_control_with_completion(route, None)
    }

    /// Install the completion registry a private instance owns.
    ///
    /// Once only. A second install would leave client writers registered
    /// before it reporting outcomes to a registry nobody reads.
    fn install_control_completion(&self, completion: ControlCompletionRegistry) -> bool {
        self.control_completion.set(completion).is_ok()
    }

    /// The completion registry a client writer should report outcomes to.
    fn control_completion(&self) -> Option<ControlCompletionRegistry> {
        self.control_completion.get().cloned()
    }

    /// Record that this client's control writer has stopped.
    ///
    /// However it stopped: a stop flag, a disconnected queue, a failure
    /// partway, a full acknowledgement channel, or an unwind. A registration
    /// can outlive its writer -- returning on a full channel is exactly that
    /// -- so this is a fact about the writer, not about the registration.
    fn mark_control_writer_gone(&self, client: XServerFrontendClientId) {
        if let Ok(clients) = self.clients.lock()
            && let Some(senders) = clients.get(&client)
        {
            senders.control_writer_gone.store(true, Ordering::Release);
        }
    }

    /// Whether this client has a control writer that could still execute work.
    ///
    /// Positive: the client is registered here and its writer has not stopped.
    /// An unreadable registry answers no, because accepting work on a
    /// question nobody could answer is how work is accepted for a writer that
    /// has gone.
    fn control_writer_present(&self, client: XServerFrontendClientId) -> bool {
        self.clients
            .lock()
            .ok()
            .and_then(|clients| {
                clients
                    .get(&client)
                    .map(|senders| !senders.control_writer_gone.load(Ordering::Acquire))
            })
            .unwrap_or(false)
    }

    /// Enter a routing call as an executor for this client.
    ///
    /// A control with no registration is ungoverned and routes as it always
    /// did. One with a registration needs something already executing for its
    /// client, or there is nothing to route it to.
    fn enter_routing_execution(
        &self,
        client: XServerFrontendClientId,
        completion: Option<ControlCompletionToken>,
    ) -> Option<ControlExecutorLease> {
        match (self.control_completion.get(), completion) {
            (Some(registry), Some(_)) => registry.enter_routing(client),
            (_, None) => Some(ControlExecutorLease::ungoverned()),
            (None, Some(_)) => None,
        }
    }

    /// Claim execution of a control before anything authoritative happens.
    ///
    /// A command with no registration is ungoverned and routes as it always
    /// did. A registration with no registry to answer to is refused: nothing
    /// here could establish who owns the outcome.
    fn claim_control_execution(
        &self,
        completion: Option<ControlCompletionToken>,
    ) -> ControlExecutionClaim {
        match (self.control_completion.get(), completion) {
            (Some(registry), Some(token)) => registry.claim_execution(token),
            (_, None) => ControlExecutionClaim::Ungoverned,
            (None, Some(_)) => {
                ControlExecutionClaim::Refused(ControlClaimRefusal::Unavailable)
            }
        }
    }

    /// Route a control, carrying a completion registration when the private
    /// path made one.
    ///
    /// Both producer routes take the token. `route_focus_control` returns
    /// early below for focus commands, so attaching it only at the
    /// construction further down would cover ordinary control and silently
    /// miss focus.
    fn route_control_with_completion(
        &self,
        route: XAuthorityClientControlCommand,
        completion: Option<ControlCompletionToken>,
    ) -> Result<(), XServerFrontendRouteError> {
        if !self.input_recovery.active(None, route.client) {
            return Err(XServerFrontendRouteError::UnknownClient { client: route.client });
        }
        // Taken before the first authoritative effect and held across all of
        // them, because routing is one of them: focus routing sends FocusOut
        // and moves the focused surface before any writer runs. Holding it is
        // what stops a writer's exit abandoning an operation this call is
        // still inside.
        //
        // Taking it is also the liveness check, so there is no gap between
        // deciding this client has an executor and being one. A separate
        // precheck could be true and then false before the claim.
        let Some(_executing) = self.enter_routing_execution(route.client, completion) else {
            return Err(XServerFrontendRouteError::UnknownClient { client: route.client });
        };
        // Before the first authoritative effect, which is not the writer.
        // Focus routing sends FocusOut to the previously focused client and
        // moves the focused surface before any writer runs, so a claim taken
        // at the writer would leave those effects behind a record still
        // reporting the operation unexecuted.
        if !self.claim_control_execution(completion).permits_effects() {
            return Err(XServerFrontendRouteError::ControlNotClaimable {
                client: route.client,
            });
        }
        if let Some(result) = self.route_focus_control(route, completion) {
            return result;
        }
        let senders = self.client_senders(route.client)?;
        let incarnation = senders.connection_state.clone();
        self.route_to_client(
            route.client,
            &incarnation,
            senders.control,
            X11RoutedControl::Authority {
                command: route.command,
                focus: None,
                claim: None,
                completion,
            },
        )
    }

    fn acknowledge_stale_control(
        &self,
        route: XAuthorityClientControlCommand,
    ) -> Result<(), XServerFrontendRouteError> {
        match self
            .acknowledgement_sender
            .try_send(XAuthorityClientControlAck {
                client: route.client,
                acknowledgement: XAuthorityControlAck {
                    kind: route.command.kind(),
                    transaction: route.command.transaction(),
                    surface: route.command.surface(),
                    outcome: XAuthorityControlOutcome::ClientGone,
                },
            }) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => Ok(()),
            Err(TrySendError::Full(_)) => {
                Err(XServerFrontendRouteError::ClientQueueFull {
                    client: route.client,
                })
            }
        }
    }

    fn route_protocol(
        &self,
        client: XServerFrontendClientId,
        event: XClientEvent,
    ) -> Result<(), XServerFrontendRouteError> {
        self.route_protocol_to_watcher(client, event)
            .map_err(|refusal| match refusal {
                XServerFrontendWatcherRefusal::Stalled(stalled) => {
                    XServerFrontendRouteError::ClientQueueFull {
                        client: stalled.client,
                    }
                }
                XServerFrontendWatcherRefusal::Route(error) => error,
            })
    }

    /// As `route_protocol`, but a recipient that has stopped draining is named
    /// exactly.
    ///
    /// THE NUMBER IN A `ClientQueueFull` IS NOT ENOUGH TO ACT ON. The caller
    /// that ends a stalled watcher does so after this returns, and by then the
    /// number can belong to a successor. What comes back here is the identity
    /// this route captured with the sender it used, so the ending reaches the
    /// connection that stalled and not whoever holds its number next.
    fn route_protocol_to_watcher(
        &self,
        client: XServerFrontendClientId,
        event: XClientEvent,
    ) -> Result<(), XServerFrontendWatcherRefusal> {
        let (incarnation, sender) = match self.client_senders(client) {
            Ok(senders) => (senders.connection_state.clone(), senders.protocol),
            Err(XServerFrontendRouteError::UnknownClient { .. }) => return Ok(()),
            Err(error) => return Err(XServerFrontendWatcherRefusal::Route(error)),
        };
        match self.route_to_client(client, &incarnation, sender, event) {
            Ok(()) => Ok(()),
            Err(
                XServerFrontendRouteError::UnknownClient { .. }
                | XServerFrontendRouteError::ClientQueueDisconnected { .. },
            ) => Ok(()),
            Err(XServerFrontendRouteError::ClientQueueFull { .. }) => {
                Err(XServerFrontendWatcherRefusal::Stalled(
                    XServerFrontendStalledRecipient {
                        client,
                        occupant: incarnation,
                    },
                ))
            }
            Err(error) => Err(XServerFrontendWatcherRefusal::Route(error)),
        }
    }

    fn client_senders(
        &self,
        client: XServerFrontendClientId,
    ) -> Result<XServerFrontendClientRouteSenders, XServerFrontendRouteError> {
        self.clients
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .get(&client)
            .cloned()
            .ok_or(XServerFrontendRouteError::UnknownClient { client })
    }

    /// THE REMOVAL IS THE SENDER'S CONNECTION'S, NOT THE NUMBER'S. A send can
    /// fail long after the connection that handed out this sender has gone,
    /// and by then the number may be somebody else's. Removing by number then
    /// would revoke a connection nothing was wrong with.
    fn route_to_client<T>(
        &self,
        client: XServerFrontendClientId,
        incarnation: &Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
        sender: SyncSender<T>,
        value: T,
    ) -> Result<(), XServerFrontendRouteError> {
        match sender.try_send(value) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                Err(XServerFrontendRouteError::ClientQueueFull { client })
            }
            Err(TrySendError::Disconnected(_)) => {
                self.remove_row_of(client, incarnation)?;
                Err(XServerFrontendRouteError::ClientQueueDisconnected { client })
            }
        }
    }

    /// Remove this number's row only while it is still this connection's.
    ///
    /// ASKED AND ACTED ON UNDER THE ONE ACQUISITION, so a successor published
    /// between a check and a removal is not the one removed.
    fn remove_row_of(
        &self,
        client: XServerFrontendClientId,
        incarnation: &Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
    ) -> Result<bool, XServerFrontendRouteError> {
        let mut clients = self
            .clients
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let ours = clients
            .get(&client)
            .is_some_and(|row| Arc::ptr_eq(&row.connection_state, incarnation));
        if ours {
            clients.remove(&client);
        }
        Ok(ours)
    }

    fn registered_client_count(&self) -> usize {
        self.clients
            .lock()
            .map(|clients| clients.len())
            .unwrap_or(0)
    }

    fn send_input_delivery(
        &self,
        client: XServerFrontendClientId,
        delivery: Option<XAuthorityInputDeliveryId>,
        outcome: XAuthorityInputDeliveryOutcome,
    ) -> Result<(), XServerFrontendRouteError> {
        self.input_recovery.finish(client, delivery, outcome)
    }

    fn send_route_lease_update(
        &self,
        identity: sophia_protocol::ApplicationRouteLeaseIdentity,
        target_surface: SurfaceId,
        admission: ClientAdmissionContext,
        kind: XAuthorityRouteLeaseUpdateKind,
    ) -> Result<(), XServerFrontendRouteError> {
        let Some(sender) = self.route_lease_update_sender.as_ref() else {
            return Ok(());
        };
        let _ = sender.send(XAuthorityRouteLeaseUpdate {
            identity,
            target_surface,
            admission,
            kind,
        });
        Ok(())
    }
}

#[cfg(unix)]
impl Drop for XServerFrontendClientRouteRegistration {
    /// STILL THE ONLY PRODUCTION TRIGGER, at the same point. What it runs is
    /// now decided rather than assumed.
    ///
    /// A REGISTRATION WITH NO PRIVATE SOURCE RUNS WHAT IT ALWAYS RAN: the
    /// public path, and a private connection that holds no place, have no
    /// registered start to close and nothing that could have been started,
    /// so the synchronous body runs here in the same order as before.
    ///
    /// A REGISTRATION WITH ONE DECIDES FIRST. Its startup admission is closed
    /// and an admitted start is told to stop before anything is waited for;
    /// the synchronous body runs only if that decision establishes that
    /// nothing was ever started. Otherwise the duty and the number stay with
    /// the custody's keeper, recorded as deferred, and this frame returns
    /// without touching the home, the gate, the place or the tables. Nothing
    /// here joins.
    fn drop(&mut self) {
        match self.ordered_custody.as_ref() {
            None => self.cleanup.run_synchronous_cleanup(),
            Some(registered) => self.destroy_registered(registered),
        }
    }
}
