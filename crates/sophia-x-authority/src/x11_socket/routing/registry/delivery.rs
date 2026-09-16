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
        let sender = self.client_senders(route.client)?.control;
        self.route_to_client(
            route.client,
            sender,
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
        let sender = match self.client_senders(client) {
            Ok(senders) => senders.protocol,
            Err(XServerFrontendRouteError::UnknownClient { .. }) => return Ok(()),
            Err(error) => return Err(error),
        };
        match self.route_to_client(client, sender, event) {
            Err(
                XServerFrontendRouteError::UnknownClient { .. }
                | XServerFrontendRouteError::ClientQueueDisconnected { .. },
            ) => Ok(()),
            result => result,
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

    fn route_to_client<T>(
        &self,
        client: XServerFrontendClientId,
        sender: SyncSender<T>,
        value: T,
    ) -> Result<(), XServerFrontendRouteError> {
        match sender.try_send(value) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                Err(XServerFrontendRouteError::ClientQueueFull { client })
            }
            Err(TrySendError::Disconnected(_)) => {
                self.clients
                    .lock()
                    .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                    .remove(&client);
                Err(XServerFrontendRouteError::ClientQueueDisconnected { client })
            }
        }
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
impl XServerFrontendClientRouteRegistration {
    /// Close this endpoint and move what it still owes into its own place.
    ///
    /// THE FENCE IS WHAT MAKES THE MOVE SOUND. Once it is established no
    /// further capsule can be accepted for this connection, so the queue taken
    /// here is the whole of what was accepted -- not a snapshot with a
    /// producer still writing behind it.
    ///
    /// WHAT IS RETAINED IS NOT UNPACKED. The receiver, or the transport if
    /// binding got that far, moves whole. Nothing is received off the queue
    /// here: receiving is how a driver learns whether producers are gone, and
    /// doing it during teardown would mean deciding the disposition of
    /// accepted work on the path that is least able to answer for it.
    ///
    /// THE SOCKET GOES WITH THE TRANSPORT, OR IT DOES NOT GO AT ALL. A
    /// transport carries an independent handle on this connection, so a
    /// retained transport keeps the wire open after the connection's own frame
    /// is gone and whoever drives it can still end it. A receiver alone
    /// carries no such handle: the accepted socket closes with the connection
    /// that owned it, and the queue is retained with no way to deliver what is
    /// in it. That is a worse outcome and it is recorded as what it is rather
    /// than smoothed over -- the refusal says why there is no transport.
    ///
    /// Ending the wire is NOT done here. A close is its own act with its own
    /// outcome, and a teardown that ended a wire in passing would report
    /// nothing about whether it worked.
    fn retain_ordered_continuation(&self) {
        let fence = self.fence_ordered_handovers();
        // The place was taken before this connection was exposed. Taking the
        // slot out is what says this registration is done with it.
        let slot = match self.ordered_continuation.lock() {
            Ok(mut held) => held.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        let custody = match self.ordered_setup.lock() {
            Ok(mut held) => held.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        let Some(slot) = slot else {
            // No place. Either this registry has no continuation store, or the
            // place has already been disposed of; in both cases there is
            // nothing to install into and nothing here may invent one.
            drop(custody);
            return;
        };
        let Some(accepted) = custody else {
            // NO CUSTODY IS NOT NO WORK. This connection's row was published,
            // so capsules may have been accepted into a queue whose receiver
            // went somewhere this registration cannot see -- setup may never
            // have reached the binding, or the receiver may be held by whoever
            // called. The disposition of anything on it is unknown, and
            // unknown is retained: the slot's own drop accounts for the place
            // rather than returning it as though nothing had been exposed.
            drop(slot);
            return;
        };
        // Source-owned until the destination is held: `install` takes it out
        // of this slot only once it has somewhere to put it.
        let mut source = Some(PrivateOrderedContinuation::Setup {
            accepted,
            // No owner was ever built. Not a failure -- attaching a worker is
            // separate work that has not landed -- and the honest reason there
            // is nobody to answer for what is on this queue.
            refusal: X11OrderedServingRefusal::Unserved,
            // Nothing was received here, so nothing is retained beside the
            // queue and nothing is known about its producers.
            retained: Vec::new(),
            drained: false,
            // A teardown does not end a wire, so it claims neither.
            ended: false,
            ending_refused: None,
        });
        slot.install(&mut source);
        debug_assert!(
            source.is_none(),
            "an installed continuation leaves its source empty"
        );
        let _ = fence;
    }
}

#[cfg(unix)]
impl Drop for XServerFrontendClientRouteRegistration {
    fn drop(&mut self) {
        // FIRST, AND BEFORE THE ROW GOES. Closing this endpoint to further
        // handovers is what makes everything after it well defined: a producer
        // that already captured this connection's sender is refused from here,
        // and one that was already inside finishes before this returns. Taking
        // the queue away first would leave a producer to accept into a queue
        // nobody would look at again.
        //
        // Removing the row is not a substitute and never was. The capture
        // happens under the client table and the send happens after it is
        // released, so a row removed here says nothing about a sender already
        // in someone's hand.
        self.retain_ordered_continuation();
        if let Ok(Some(lease)) = self.lifecycle.get_mut() {
            lease.close();
        }
        // Before the route senders go. What was mid-application when the
        // client's registration ended is not unexecuted and is not answered;
        // it is owed the cleanup it named, and saying so here is what keeps
        // that responsibility from ending with the registration.
        if let Some(completion) = self.control_completion.get() {
            // A writer expected for this client is not coming now. If a writer
            // did start, this changes nothing and its own exit is the edge;
            // if startup failed before one ever existed, this is what stops an
            // expectation nothing will meet from keeping the client executing
            // forever. Either way the registry decides what that leaves,
            // under the lock that abandons.
            completion.cancel_expected_writer(self.client);
        }
        let _ = self.input_recovery.disconnect(self.client, XAuthorityInputDeliveryOutcome::ClientDisconnected);
        if let Ok(mut clients) = self.clients.lock() {
            clients.remove(&self.client);
        }
        if let Ok(mut surfaces) = self.surfaces.lock() {
            surfaces.retain(|_, route| route.client != self.client);
        }
        if let Ok(mut focused) = self.focused_surface.lock()
            && focused.is_some_and(|route| route.client == self.client)
        {
            *focused = None;
        }
        if let Ok(mut parents) = self.window_parents.lock() {
            parents.retain(|(client, _), _| *client != self.client);
        }
        if let Ok(mut subscriptions) = self.core_event_subscriptions.lock() {
            subscriptions.retain(|(client, _), _| *client != self.client);
        }
        if let Ok(mut subscriptions) = self.randr_subscriptions.lock() {
            subscriptions.remove(&self.client);
        }
        // Retired here as well as on an orderly close, because a client whose
        // connection failed before that point never reaches it. A client id
        // may be reissued, and an inherited subscription would deliver one
        // client's selections to whoever takes the id next.
        if let Ok(mut subscriptions) = self.xfixes_selection_subscriptions.lock() {
            subscriptions.retain(|(client, _, _), _| *client != self.client);
        }
        if let Ok(mut subscriptions) = self.present_subscriptions.lock() {
            subscriptions.retain(|(client, _), _| *client != self.client);
        }
        if let Ok(mut pending) = self.pending_presentations.entries.lock() {
            pending.retain(|_, presentation| presentation.client != self.client);
            self.pending_presentations.capacity_changed.notify_all();
        }
        let abandoned = if let Ok(mut frozen) = self.frozen_input.lock() {
            let (abandoned, retained): (Vec<_>, Vec<_>) = frozen.drain(..)
                .partition(|route| route.client == self.client);
            *frozen = retained.into();
            abandoned
        } else { Vec::new() };
        for route in abandoned {
            let _ = self.input_recovery.finish(self.client, route.route.delivery,
                XAuthorityInputDeliveryOutcome::ClientDisconnected);
        }
    }
}
