#[cfg(unix)]
const XI_POINTER_EMULATED: u32 = 1 << 16;

#[cfg(unix)]
fn clamp_input_coordinate(value: f64) -> i16 {
    if !value.is_finite() {
        return 0;
    }
    value
        .floor()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

#[cfg(unix)]
fn encode_xi_device_event(
    byte_order: XByteOrder,
    sequence: u16,
    event_type: u16,
    event: XAuthorityInputEvent,
    event_window: XResourceId,
    child_window: XResourceId,
    event_x: i16,
    event_y: i16,
    flags: u32,
) -> Vec<u8> {
    encode_xi_device_frame(byte_order, sequence, event_type, event, event_window,
        child_window, event_x, event_y, flags).as_bytes().to_vec()
}

#[cfg(unix)]
fn encode_xi_crossing_event(
    byte_order: XByteOrder,
    sequence: u16,
    event_type: u16,
    event: XAuthorityInputEvent,
    event_window: XResourceId,
) -> Vec<u8> {
    encode_xi_crossing_frame(byte_order, sequence, event_type, event, event_window)
        .as_bytes().to_vec()
}

#[cfg(unix)]
fn write_xi_u16(byte_order: XByteOrder, out: &mut [u8], value: u16) {
    match byte_order {
        XByteOrder::LittleEndian => out.copy_from_slice(&value.to_le_bytes()),
        XByteOrder::BigEndian => out.copy_from_slice(&value.to_be_bytes()),
    }
}

#[cfg(unix)]
fn write_xi_u32(byte_order: XByteOrder, out: &mut [u8], value: u32) {
    match byte_order {
        XByteOrder::LittleEndian => out.copy_from_slice(&value.to_le_bytes()),
        XByteOrder::BigEndian => out.copy_from_slice(&value.to_be_bytes()),
    }
}

#[cfg(unix)]
enum X11InputEventReceiver {
    Plain(Receiver<XAuthorityInputEvent>),
    Routed {
        receiver: Receiver<XAuthorityClientInputEvent>,
        deliveries: Option<Sender<XAuthorityClientInputDelivery>>,
        recovery: Option<InputRecovery>,
    },
}

#[cfg(unix)]
type X11ReceivedInputEvent = (
    XAuthorityInputEvent,
    Option<XResourceId>,
    Option<u16>,
    Option<XResourceId>,
    Option<u16>,
    Option<XResourceId>,
    u16,
    Option<XAuthorityInputDeliveryId>,
);

#[cfg(unix)]
impl X11InputEventReceiver {
    fn recv_timeout(
        &self,
        client: XServerFrontendClientId,
    ) -> Result<X11ReceivedInputEvent, RecvTimeoutError> {
        match self {
            Self::Plain(receiver) => receiver
                .recv_timeout(Duration::from_millis(10))
                .map(|event| (event, None, None, None, None, None, 0, None)),
            Self::Routed { receiver, .. } => {
                match receiver.recv_timeout(Duration::from_millis(10)) {
                    Ok(route) if route.client == client => Ok((
                        route.event,
                        route.target_window,
                        route.xi_event_type,
                        route.xi_event_window,
                        route.xi_emulated_button_type,
                        route.xi_emulated_button_window,
                        route.xi_pointer_crossing_mask,
                        route.delivery,
                    )),
                    // Drop one misaddressed route, then let the writer loop
                    // observe its stop flag before it receives again.
                    Ok(route) => {
                        let _ = self.send_delivery(route.client, route.delivery,
                            XAuthorityInputDeliveryOutcome::RouteRejected);
                        Err(RecvTimeoutError::Timeout)
                    },
                    Err(error) => Err(error),
                }
            }
        }
    }

    fn send_delivery(
        &self,
        client: XServerFrontendClientId,
        delivery: Option<XAuthorityInputDeliveryId>,
        outcome: XAuthorityInputDeliveryOutcome,
    ) -> Result<(), X11SetupSocketError> {
        if let Self::Routed { recovery: Some(recovery), .. } = self {
            return recovery.finish(client, delivery, outcome)
                .map_err(|error| X11SetupSocketError::new(error.to_string()));
        }
        let Some(delivery) = delivery else { return Ok(()); };
        let Self::Routed {
            deliveries: Some(sender),
            ..
        } = self
        else {
            return Ok(());
        };
        match sender.send(XAuthorityClientInputDelivery {
            client,
            delivery,
            outcome,
        }) {
            Ok(()) | Err(_) => Ok(()),
        }
    }
}

#[cfg(unix)]
enum X11ControlChannels {
    Routed {
        receiver: Receiver<XAuthorityClientControlCommand>,
        acknowledgements: SyncSender<XAuthorityClientControlAck>,
        /// Present only on a private instance, where every accepted control
        /// has a registration waiting for its outcome.
        completion: Option<ControlCompletionRegistry>,
    },
    ClientBound {
        receiver: Receiver<X11RoutedControl>,
        acknowledgements: SyncSender<XAuthorityClientControlAck>,
        completion: Option<ControlCompletionRegistry>,
    },
}

#[cfg(unix)]
impl X11ControlChannels {
    fn recv_timeout(
        &self,
        client: XServerFrontendClientId,
    ) -> Result<X11RoutedControl, RecvTimeoutError> {
        match self {
            Self::Routed { receiver, .. } => {
                match receiver.recv_timeout(Duration::from_millis(10)) {
                    Ok(route) if route.client == client => Ok(X11RoutedControl::Authority {
                        command: route.command,
                        focus: None,
                        claim: None,
                        // This path takes a command straight off the shared
                        // receiver rather than from a private producer, so
                        // there is no registration to carry.
                        completion: None,
                    }),
                    // Drop one misaddressed route, then let the writer
                    // loop observe its stop flag before it receives again.
                    Ok(_) => Err(RecvTimeoutError::Timeout),
                    Err(error) => Err(error),
                }
            }
            Self::ClientBound { receiver, .. } => receiver.recv_timeout(Duration::from_millis(10)),
        }
    }

    fn completion(&self) -> Option<&ControlCompletionRegistry> {
        match self {
            Self::Routed { completion, .. } | Self::ClientBound { completion, .. } => {
                completion.as_ref()
            }
        }
    }

    /// Send, and say which of the three things happened.
    ///
    /// Delivered, retained because the channel is full, or not published at
    /// all because the receiver is gone. The last two are different facts and
    /// neither is a delivery.
    fn emit_ack(&self, acknowledgement: XAuthorityClientControlAck) -> ControlPublication {
        match self {
            Self::Routed {
                acknowledgements, ..
            }
            | Self::ClientBound {
                acknowledgements, ..
            } => match acknowledgements.try_send(acknowledgement) {
                Ok(()) => ControlPublication::Delivered,
                Err(TrySendError::Disconnected(_)) => ControlPublication::ReceiverGone,
                Err(TrySendError::Full(_)) => ControlPublication::Retained,
            },
        }
    }

    /// Ask whether this writer may go on to apply a control.
    ///
    /// A writer is a continuation, not a beginning: routing the command into
    /// this queue was already an authoritative effect, and the claim was taken
    /// there. Everything after this can leave the runtime changed with no
    /// acknowledgement sent, which is exactly the state that must not later be
    /// reported as unexecuted -- so the answer is checked, not announced.
    fn resume_execution(
        &self,
        token: Option<ControlCompletionToken>,
    ) -> ControlExecutionClaim {
        match (self.completion(), token) {
            (Some(registry), Some(token)) => registry.resume_execution(token),
            // No registration was made for this operation, so no record
            // governs it. The ordinary path works exactly as before.
            (_, None) => ControlExecutionClaim::Ungoverned,
            // A registration with no registry to answer to. Nothing here can
            // establish who owns the outcome, so nothing here may produce one.
            (None, Some(_)) => {
                ControlExecutionClaim::Refused(ControlClaimRefusal::Unavailable)
            }
        }
    }

    /// Report that this operation has reached one step.
    ///
    /// Failing to record that a step is beginning prevents the step: an
    /// effect nobody recorded the intent for cannot afterwards be told from
    /// one that never happened.
    fn record_progress(
        &self,
        token: Option<ControlCompletionToken>,
        progress: ControlProgress,
    ) -> Result<(), ControlProgressRefusal> {
        match (self.completion(), token) {
            (Some(registry), Some(token)) => registry.record_progress(token, progress),
            // No record governs this operation, so there is nothing to report
            // to and nothing gating the effect.
            (_, None) => Ok(()),
            // A registration whose registry cannot be reached. The effect must
            // not happen: an effect nothing recorded the intent for cannot be
            // told afterwards from one that never happened.
            (None, Some(_)) => Err(ControlProgressRefusal::Unavailable),
        }
    }


    /// Publish an acknowledgement against a private completion registration.
    ///
    /// The registration authorises the send and the send happens under the
    /// same hold, so an acknowledgement the record refuses is refused before
    /// anyone outside can see it. Sending first and reporting afterwards
    /// refused nothing: a contradicting or duplicate acknowledgement was
    /// already at the receiver, and no later verdict could recall it.
    ///
    /// The command is never replayed from here. Whatever its effect was, it
    /// has already happened; only the acknowledgement is retained.
    fn send_ack_for(
        &self,
        client: XServerFrontendClientId,
        acknowledgement: XAuthorityControlAck,
        token: Option<ControlCompletionToken>,
    ) -> Result<(), X11SetupSocketError> {
        let owned = XAuthorityClientControlAck {
            client,
            acknowledgement,
        };
        let publication = match (self.completion(), token) {
            (Some(registry), Some(token)) => {
                match registry.publish_with(token, owned, |owned| self.emit_ack(*owned)) {
                    Ok(publication) => publication,
                    // Nothing was sent. Another owner holds this operation's
                    // outcome, so there is nothing here to deliver and nothing
                    // to retain.
                    Err(refusal) => {
                        return Err(X11SetupSocketError::new(format!(
                            "X11 control acknowledgement refused by its completion record: \
                             {refusal:?}"
                        )));
                    }
                }
            }
            // No registration governs it, exactly as the ordinary path has
            // always been.
            (_, None) => self.emit_ack(owned),
            // A registration whose registry cannot be reached. Nothing here
            // can establish whether this acknowledgement may be published.
            (None, Some(_)) => {
                return Err(X11SetupSocketError::new(
                    "X11 control acknowledgement has a registration with no registry",
                ));
            }
        };
        match publication {
            // A gone receiver has always been tolerated here, and that stays
            // the ordinary behaviour: the writer is not failed because nobody
            // is listening. It is reported precisely to the registry above,
            // because a caller that sees only Ok cannot tell publication from
            // the receiver having disappeared.
            ControlPublication::Delivered | ControlPublication::ReceiverGone => Ok(()),
            ControlPublication::Retained => Err(X11SetupSocketError::new(
                "X11 control acknowledgement channel is full",
            )),
        }
    }
}

#[cfg(unix)]
impl From<XAuthorityKeyEvent> for XAuthorityInputEvent {
    fn from(event: XAuthorityKeyEvent) -> Self {
        Self::Key(event)
    }
}

#[cfg(unix)]
impl XServerFrontendRouteRegistry {
    fn route_resolved_input(
        &self,
        namespace: NamespaceId,
        client: XServerFrontendClientId,
        surface_window: XResourceId,
        target_window: Option<XResourceId>,
        event: XAuthorityInputEvent,
        delivery: Option<XAuthorityInputDeliveryId>,
    ) -> Result<(), XServerFrontendRouteError> {
        // This is logical input already admitted past epoch and freeze checks.
        // Publish it before subscription filtering or a possibly stalled writer.
        self.input_authority.lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .observe_query_input(namespace, surface_window, event);
        let (xi_device, selected_type) = match event {
            XAuthorityInputEvent::Key(key) => (3, Some(if key.pressed { 2 } else { 3 })),
            XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                kind: XAuthorityPointerEventKind::Button { pressed, .. },
                ..
            }) => (2, Some(if pressed { 4 } else { 5 })),
            XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                kind:
                    XAuthorityPointerEventKind::Axis {
                        horizontal_position_v120,
                        vertical_position_v120,
                        ..
                    },
                ..
            }) => (
                2,
                (horizontal_position_v120.is_some() || vertical_position_v120.is_some())
                    .then_some(6),
            ),
            XAuthorityInputEvent::Pointer(_) => (2, Some(6)),
        };
        let event_window = target_window.unwrap_or(surface_window);
        let event_ancestry = self.window_ancestry(client, event_window)?;
        let xi_event_window = if let Some(selected_type) = selected_type {
            let authority = self
                .input_authority
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
            (xi_device == 2)
                .then(|| authority.pointer_grab(namespace))
                .flatten()
                .filter(|grab| {
                    grab.owner == client.raw() && grab.selects_xi_event(selected_type)
                })
                .map(|grab| grab.window)
                .or_else(|| {
                    event_ancestry
                        .iter()
                        .find(|window| {
                            authority.xi_event_selected(
                                namespace,
                                client.raw(),
                                **window,
                                xi_device,
                                selected_type,
                            )
                        })
                        .copied()
                })
        } else {
            None
        };
        let xi_event_type = xi_event_window.and(selected_type);
        let xi_emulated_button_selected_type = match event {
            XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                kind: XAuthorityPointerEventKind::Axis { pressed, .. },
                ..
            }) => Some(if pressed { 4 } else { 5 }),
            _ => None,
        };
        let xi_emulated_button_window =
            if let Some(selected_type) = xi_emulated_button_selected_type {
                let authority = self
                    .input_authority
                    .lock()
                    .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
                authority
                    .pointer_grab(namespace)
                    .filter(|grab| {
                        grab.owner == client.raw() && grab.selects_xi_event(selected_type)
                    })
                    .map(|grab| grab.window)
                    .or_else(|| {
                        event_ancestry
                            .iter()
                            .find(|window| {
                                authority.xi_event_selected(
                                    namespace,
                                    client.raw(),
                                    **window,
                                    xi_device,
                                    selected_type,
                                )
                            })
                            .copied()
                    })
            } else {
                None
            };
        let xi_emulated_button_type =
            xi_emulated_button_window.and(xi_emulated_button_selected_type);
        let transition_types: &[u16] = if xi_device == 2 { &[7, 8] } else { &[] };
        let authority = self
            .input_authority
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let xi_pointer_crossing_mask = transition_types.iter().fold(0u16, |mask, event_type| {
            if event_ancestry.iter().any(|window| {
                authority.xi_event_selected(
                    namespace,
                    client.raw(),
                    *window,
                    xi_device,
                    *event_type,
                )
            }) {
                mask | (1 << event_type)
            } else {
                mask
            }
        });
        // Queue saturation can revoke this client and clean its grabs. Do
        // not carry the subscription-read lock into that recovery path.
        drop(authority);
        let route = XAuthorityClientInputEvent {
            client,
            event,
            target_window,
            xi_event_type,
            xi_event_window,
            xi_emulated_button_type,
            xi_emulated_button_window,
            xi_pointer_crossing_mask,
            delivery,
        };
        match self.route_input(route) {
            Ok(()) => Ok(()),
            Err(error) => {
                tracing::warn!("sophia_x11_input_route status=rejected reason={error:?} content=redacted");
                self.send_input_delivery(
                    client,
                    delivery,
                    XAuthorityInputDeliveryOutcome::RouteRejected,
                )?;
                Err(error)
            }
        }
    }

    // Compatibility ingress already supplies X input rather than an Engine
    // packet. It must publish query state too, without running XKB twice.
    fn observe_direct_query_input(
        &self,
        route: &XAuthorityClientInputEvent,
    ) -> Result<(), XServerFrontendRouteError> {
        let source = {
            let surfaces = self.surfaces.lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
            match route.event {
                XAuthorityInputEvent::Pointer(pointer) => surfaces.get(&pointer.surface).copied(),
                XAuthorityInputEvent::Key(_) => surfaces.values()
                    .find(|surface| surface.client == route.client).copied(),
            }
        };
        if let Some(source) = source {
            self.input_authority.lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                .observe_query_input(source.namespace, source.window, route.event);
        }
        Ok(())
    }

    fn route_is_frozen(
        &self,
        route: &XAuthorityRoutedInput,
        namespace: NamespaceId,
    ) -> Result<bool, XServerFrontendRouteError> {
        let authority = self
            .input_authority
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        Ok(match route.request.kind {
            InputEventKind::Key { .. } => authority.keyboard_frozen(namespace),
            InputEventKind::PointerMotion
            | InputEventKind::PointerButton { .. }
            | InputEventKind::PointerAxis { .. } => authority.pointer_frozen(namespace),
            // An announcement is never frozen: it is not delivered, so there
            // is nothing a grab could hold back.
            InputEventKind::DeviceAdded { .. } | InputEventKind::DeviceRemoved => false,
        })
    }

    /// Re-route what a grab had frozen.
    ///
    /// Each item is validated again here against the stamp it was given, not
    /// against whatever is current. A thaw is a second chance to execute, so
    /// it is also a second place a revoked revision could slip through.
    fn drain_thawed_input(
        &self,
        current_control_epoch: u64,
        gate: Option<&crate::ControlEpochGate>,
    ) -> Result<usize, XServerFrontendRouteError> {
        let queued = self
            .frozen_input
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .len();
        let mut routed = 0usize;
        for _ in 0..queued {
            let deferred = self
                .frozen_input
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                .pop_front();
            let Some(deferred) = deferred else { break };
            let route = deferred.route;
            // Cancellation must consume its tombstone even while the grab
            // remains frozen; waiting for a thaw would leak queue credit.
            if !self.input_recovery.begin_routing(route.delivery) { continue; }
            let surface_route = self
                .surfaces
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                .get(&route.request.target_surface)
                .copied();
            let Some(surface_route) = surface_route else {
                tracing::warn!("sophia_x11_input_route status=rejected reason=deferred_target_gone client={} content=redacted", deferred.client.raw());
                self.send_input_delivery(
                    deferred.client,
                    route.delivery,
                    XAuthorityInputDeliveryOutcome::RouteRejected,
                )?;
                continue;
            };
            if self.route_is_frozen(&route, surface_route.namespace)? {
                self.frozen_input
                    .lock()
                    .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                    .push_back(XDeferredRoutedInput {
                        client: deferred.client,
                        control_epoch: deferred.control_epoch,
                        publication: deferred.publication,
                        route,
                    });
            } else {
                let stamp = crate::ControlStamp {
                    control_epoch: deferred.control_epoch,
                    publication: deferred.publication,
                };
                let admitted = match gate {
                    Some(gate) => gate.admits(stamp).is_ok(),
                    None => deferred.control_epoch == current_control_epoch,
                };
                self.route_engine_input_admitted(route, stamp, admitted)?;
                routed = routed.saturating_add(1);
            }
        }
        Ok(routed)
    }
}
