#[cfg(unix)]
struct X11ClientConnectionInputs {
    input_receiver: Option<X11InputEventReceiver>,
    control_channels: Option<X11ControlChannels>,
    client_routing: Option<XServerFrontendRouteRegistry>,
}

#[cfg(unix)]
struct X11ClientAdmissionContext<'a> {
    authorization: &'a XServerFrontendSetupAuthorization,
    admission_policy: Option<Arc<dyn XServerFrontendAdmissionPolicy>>,
    worker_admission: Option<(u64, Sender<X11CoreClientWorkerAdmission>)>,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum X11ExplicitPointerGrabPreparation {
    Unmanaged,
    Rejected(u8),
    Prepared {
        identity: sophia_protocol::ApplicationRouteLeaseIdentity,
        anchor: crate::XAuthorityExplicitPointerGrabAnchor,
        replaces: Option<sophia_protocol::ApplicationRouteLeaseIdentity>,
    },
}

#[cfg(unix)]
/// Which XFixes subtype reports a selection change of this cause.
fn selection_change_subtype(kind: crate::XSelectionChangeKind) -> u8 {
    match kind {
        crate::XSelectionChangeKind::SelectionWindowDestroyed => {
            crate::X_XFIXES_SELECTION_WINDOW_DESTROY_SUBTYPE
        }
        crate::XSelectionChangeKind::SelectionClientClosed => {
            crate::X_XFIXES_SELECTION_CLIENT_CLOSE_SUBTYPE
        }
        // Setting an owner and clearing one are the same cause: the selection
        // was assigned, to a window or to nobody.
        _ => crate::X_XFIXES_SET_SELECTION_OWNER_SUBTYPE,
    }
}

#[cfg(unix)]
/// Whether a routing failure belongs to the recipient rather than to the
/// service.
///
/// A watcher that has already gone must not end everyone else's session: its
/// event is dropped and the sender carries on. A watcher that is still
/// connected but no longer draining is not covered here -- it is owed its
/// events, so it is disconnected rather than quietly skipped. Shared state
/// failing is a different thing again and stays fatal.
fn x11_recipient_is_gone(error: &XServerFrontendRouteError) -> bool {
    matches!(
        error,
        XServerFrontendRouteError::ClientQueueDisconnected { .. }
            | XServerFrontendRouteError::UnknownClient { .. }
    )
}

#[cfg(unix)]
fn x11_explicit_pointer_grab_client_error(
    error: crate::XAuthorityExplicitPointerGrabBridgeError,
) -> X11SetupSocketError {
    X11SetupSocketError::client_failure(format!(
        "explicit pointer-grab arbitration failed: {error:?}"
    ))
}

#[cfg(unix)]
fn x11_prepare_explicit_pointer_grab(
    state: &X11CoreSocketServerState,
    routing: Option<&XServerFrontendRouteRegistry>,
    admission: Option<ClientAdmissionContext>,
    namespace: NamespaceId,
    client: XServerFrontendClientId,
    request: &crate::XWireRequest,
    after_observation: Option<TransactionId>,
) -> Result<X11ExplicitPointerGrabPreparation, X11SetupSocketError> {
    // The virtual source cannot be detached or grabbed independently. Let the
    // dispatcher return BadAccess without reserving the Engine's master route.
    if matches!(
        request,
        crate::XWireRequest::XiGrabDevice {
            device_id: crate::X_INPUT_POINTER_SOURCE_ID,
            ..
        }
    ) {
        return Ok(X11ExplicitPointerGrabPreparation::Unmanaged);
    }
    let Some(control) = routing.and_then(|routing| routing.explicit_pointer_grabs.as_ref()) else {
        return Ok(X11ExplicitPointerGrabPreparation::Unmanaged);
    };
    let Some(admission) = admission else {
        return Ok(X11ExplicitPointerGrabPreparation::Rejected(1));
    };
    let (window, pointer_mode, keyboard_mode, cursor) = match request {
        crate::XWireRequest::GrabPointer {
            window,
            pointer_mode,
            keyboard_mode,
            ..
        } => (*window, *pointer_mode, *keyboard_mode, None),
        crate::XWireRequest::XiGrabDevice {
            window,
            cursor,
            device_id: 2,
            pointer_mode,
            keyboard_mode,
            ..
        } => (*window, *pointer_mode, *keyboard_mode, *cursor),
        crate::XWireRequest::XiGrabDevice { .. } => {
            return Ok(X11ExplicitPointerGrabPreparation::Rejected(1));
        }
        _ => return Ok(X11ExplicitPointerGrabPreparation::Unmanaged),
    };
    let control_epoch = routing
        .expect("control has a route registry")
        .input_control_epoch
        .load(Ordering::Acquire);
    let runtime = lock_x11_request_runtime(&state.runtime, &state.control_runtime_pending)?;
    if pointer_mode > 1
        || keyboard_mode > 1
        || cursor.is_some_and(|cursor| runtime.validate_cursor_access(namespace, cursor).is_err())
    {
        return Ok(X11ExplicitPointerGrabPreparation::Rejected(1));
    }
    let anchor = if window.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
        crate::XAuthorityExplicitPointerGrabAnchor::AdmissionDefault
    } else {
        let Ok((_, surface, _, _)) = runtime.window_presentation_root_and_offset(namespace, window)
        else {
            return Ok(X11ExplicitPointerGrabPreparation::Rejected(3));
        };
        crate::XAuthorityExplicitPointerGrabAnchor::Surface(surface)
    };
    let active = runtime.input_authority_mut().pointer_grab(namespace);
    if active.is_some_and(|active| active.owner != client.raw()) {
        return Ok(X11ExplicitPointerGrabPreparation::Rejected(1));
    }
    let replaces = active.and_then(|active| active.route_lease);
    drop(runtime);
    let response = control.request(
        admission,
        crate::XAuthorityExplicitPointerGrabRequestKind::Prepare {
            anchor,
            replaces,
            after_observation,
            control_epoch,
        },
    );
    let response = match response {
        Ok(response) => response,
        Err(
            crate::XAuthorityExplicitPointerGrabBridgeError::Timeout
            | crate::XAuthorityExplicitPointerGrabBridgeError::Capacity,
        ) => return Ok(X11ExplicitPointerGrabPreparation::Rejected(1)),
        Err(error) => return Err(x11_explicit_pointer_grab_client_error(error)),
    };
    Ok(match response {
        crate::XAuthorityExplicitPointerGrabResponse::Prepared(identity) => {
            X11ExplicitPointerGrabPreparation::Prepared {
                identity,
                anchor,
                replaces,
            }
        }
        crate::XAuthorityExplicitPointerGrabResponse::Rejected(
            crate::XAuthorityExplicitPointerGrabRejection::NotViewable,
        ) => X11ExplicitPointerGrabPreparation::Rejected(3),
        crate::XAuthorityExplicitPointerGrabResponse::Rejected(_) => {
            X11ExplicitPointerGrabPreparation::Rejected(1)
        }
        _ => {
            return Err(X11SetupSocketError::client_failure(
                "explicit pointer-grab prepare received an invalid response",
            ));
        }
    })
}

#[cfg(unix)]
fn x11_begin_explicit_pointer_release(
    state: &X11CoreSocketServerState,
    routing: Option<&XServerFrontendRouteRegistry>,
    admission: Option<ClientAdmissionContext>,
    namespace: NamespaceId,
    client: XServerFrontendClientId,
    request: &crate::XWireRequest,
) -> Result<Option<sophia_protocol::ApplicationRouteLeaseIdentity>, X11SetupSocketError> {
    if !matches!(request, crate::XWireRequest::UngrabPointer { .. })
        && !matches!(
            request,
            crate::XWireRequest::XiUngrabDevice { device_id: 2, .. }
        )
    {
        return Ok(None);
    }
    let Some(control) = routing.and_then(|routing| routing.explicit_pointer_grabs.as_ref()) else {
        return Ok(None);
    };
    let Some(admission) = admission else {
        return Ok(None);
    };
    let runtime = lock_x11_request_runtime(&state.runtime, &state.control_runtime_pending)?;
    let Some(identity) = runtime
        .input_authority_mut()
        .pointer_grab(namespace)
        .filter(|grab| grab.owner == client.raw())
        .and_then(|grab| grab.route_lease)
    else {
        return Ok(None);
    };
    drop(runtime);
    let response = match control.request(
        admission,
        crate::XAuthorityExplicitPointerGrabRequestKind::BeginRelease { identity },
    ) {
        Ok(response) => response,
        Err(
            crate::XAuthorityExplicitPointerGrabBridgeError::Timeout
            | crate::XAuthorityExplicitPointerGrabBridgeError::Capacity,
        ) => return Ok(Some(identity)),
        Err(error) => return Err(x11_explicit_pointer_grab_client_error(error)),
    };
    match response {
        crate::XAuthorityExplicitPointerGrabResponse::ReleaseReady => Ok(Some(identity)),
        crate::XAuthorityExplicitPointerGrabResponse::Rejected(
            crate::XAuthorityExplicitPointerGrabRejection::Stale,
        ) => Ok(Some(identity)),
        _ => Err(X11SetupSocketError::client_failure(
            "explicit pointer-grab release received an invalid response",
        )),
    }
}

#[cfg(unix)]
fn x11_finish_explicit_pointer_release(
    routing: Option<&XServerFrontendRouteRegistry>,
    admission: Option<ClientAdmissionContext>,
    identity: sophia_protocol::ApplicationRouteLeaseIdentity,
) -> Result<(), X11SetupSocketError> {
    let Some(control) = routing.and_then(|routing| routing.explicit_pointer_grabs.as_ref()) else {
        return Ok(());
    };
    let Some(admission) = admission else {
        return Ok(());
    };
    let response = match control.request(
        admission,
        crate::XAuthorityExplicitPointerGrabRequestKind::FinishRelease { identity },
    ) {
        Ok(response) => response,
        Err(
            crate::XAuthorityExplicitPointerGrabBridgeError::Timeout
            | crate::XAuthorityExplicitPointerGrabBridgeError::Capacity,
        ) => return Ok(()),
        Err(error) => return Err(x11_explicit_pointer_grab_client_error(error)),
    };
    match response {
        crate::XAuthorityExplicitPointerGrabResponse::Released
        | crate::XAuthorityExplicitPointerGrabResponse::Rejected(
            crate::XAuthorityExplicitPointerGrabRejection::Stale,
        ) => Ok(()),
        _ => Err(X11SetupSocketError::client_failure(
            "explicit pointer-grab release acknowledgement was invalid",
        )),
    }
}

/// A failed post-dispatch delivery still owes the complete authority effects.
/// Only a request that never dispatched may retire an empty ordering ticket.
#[cfg(unix)]
fn failed_x11_dispatch_observation(
    pending: Option<X11DispatchObservation>,
    started: bool,
    complete: bool,
) -> Option<X11DispatchObservation> {
    pending.map(|mut observation| {
        if !complete {
            observation.failure = Some(if started {
                X11ObservedDispatchFailure::UnpublishedEffects
            } else {
                X11ObservedDispatchFailure::DispatchAborted
            });
        }
        observation
    })
}

/// Retains the last successfully admitted Sophia surface generation for each
/// client-local XID. X11 continues to address the current resource by its raw
/// XID; deferred Engine routes use this non-recyclable identity instead.
#[cfg(unix)]
#[derive(Default)]
struct X11SurfaceGenerationLedger {
    admitted: BTreeMap<u32, u32>,
}

#[cfg(unix)]
impl X11SurfaceGenerationLedger {
    fn candidate(&self, index: u32) -> Result<SurfaceId, X11SetupSocketError> {
        let generation = self
            .admitted
            .get(&index)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| {
                X11SetupSocketError::client_failure("X11 surface generation exhausted")
            })?;
        Ok(SurfaceId::new(index, generation))
    }

    fn admit(&mut self, surface: SurfaceId) -> Result<(), X11SetupSocketError> {
        if self.candidate(surface.index())? != surface {
            return Err(X11SetupSocketError::new(
                "X11 surface generation admission was not the current candidate",
            ));
        }
        self.admitted.insert(surface.index(), surface.generation());
        Ok(())
    }
}

#[cfg(unix)]
fn serve_x11_core_socket_client_with_trace_observer_and_input(
    stream: &mut UnixStream,
    namespace: NamespaceId,
    state: &X11CoreSocketServerState,
    inputs: X11ClientConnectionInputs,
    admission: X11ClientAdmissionContext<'_>,
    mut observer: impl FnMut(X11DispatchObservation) -> Result<Option<TransactionId>, X11SetupSocketError>,
) -> Result<(), X11SetupSocketError> {
    let X11ClientConnectionInputs {
        input_receiver,
        control_channels,
        client_routing,
    } = inputs;
    let X11ClientAdmissionContext {
        authorization,
        admission_policy,
        worker_admission,
    } = admission;
    let peer_credentials = if admission_policy.is_some() {
        x11_peer_credentials(stream)?
    } else {
        None
    };
    let mut setup_lease = None;
    let mut connection_state = None;
    let mut _device_pin = None;
    let mut admission_lease = None;
    let mut admission_failure = None;
    let Some((setup, setup_success)) = serve_x11_setup_socket_client_with_setup_authorization(
        stream,
        authorization,
        |setup_request| {
            let Some((setup_authentication, verified_private_input)) =
                authorization.verified_authentication(setup_request)
            else {
                return Ok(None);
            };
            if let Some(policy) = admission_policy.as_ref() {
                let request = XServerFrontendAdmissionRequest {
                    setup_authentication,
                    verified_private_input,
                    peer_credentials,
                };
                match policy.admit(request) {
                    Ok(context) if context.is_valid() => {
                        admission_lease =
                            Some(XServerFrontendAdmissionLease::new(policy.clone(), context));
                    }
                    Ok(_) => {
                        admission_failure = Some(XServerFrontendAdmissionError::Unavailable);
                        return Ok(None);
                    }
                    Err(error) => {
                        admission_failure = Some(error);
                        return Ok(None);
                    }
                }
            }
            debug_assert!(authorization.permits(setup_request));
            let (lease, setup_success) = state.next_client_setup_success()?;
            let (pinned, pin) = state.pin_connection_device(lease.client)?;
            connection_state = Some(pinned);
            _device_pin = Some(pin);
            setup_lease = Some(lease);
            Ok(Some(setup_success))
        },
    )?
    else {
        if admission_failure == Some(XServerFrontendAdmissionError::Unavailable) {
            return Err(X11SetupSocketError::new(
                "Sophia X Server Frontend admission policy unavailable",
            ));
        }
        return Ok(());
    };
    let connection_state = connection_state.ok_or_else(|| X11SetupSocketError::new("X11 connection device was not pinned"))?;
    let state = &connection_state;
    let namespace = admission_lease
        .as_ref()
        .map(|lease| lease.context().namespace.id)
        .unwrap_or(namespace);
    let client_lease = setup_lease.ok_or_else(|| {
        X11SetupSocketError::new("Sophia X Server Frontend did not retain a setup client lease")
    })?;
    let client = client_lease.client;
    // Publish the window-manager advertisement before the client can ask for it.
    // A toolkit reads it during startup, and one that finds nothing concludes
    // no manager is running and takes an unmanaged path for the rest of its
    // life. Seeded here because the namespace is only known once a connection
    // is admitted, and written under Replace so a second connection in the same
    // namespace changes nothing.
    {
        let mut atoms = state
            .atoms
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 atom table lock was poisoned"))?;
        let mut properties = state
            .properties
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 property table lock was poisoned"))?;
        crate::seed_wm_advertisement(
            &mut properties,
            &mut atoms,
            namespace,
            setup.byte_order,
        )
        .map_err(|error| {
            X11SetupSocketError::new(format!(
                "failed to publish the window manager advertisement: {error:?}"
            ))
        })?;
    }
    if std::env::var_os("SOPHIA_X11_AUTHORITY_TRACE").is_some() {
        tracing::debug!(
            "sophia_x11_client_route schema=1 stage=accepted client={}",
            client.raw()
        );
    }
    let resource_id_range = client_lease.resource_id_range;
    let mut surface_generations = X11SurfaceGenerationLedger::default();
    let mut sequence = 0u16;
    let event_sequence = Arc::new(AtomicU16::new(0));
    let focused_surface_window = Arc::new(AtomicU64::new(u64::from(X_SETUP_DEFAULT_ROOT)));
    let core_event_selections = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
    let xkb_state_details = Arc::new(AtomicU16::new(0));
    let xkb_modifiers = Arc::new(AtomicU16::new(0));
    let surface_windows = Arc::new(Mutex::new(BTreeMap::new()));
    let metadata_rules = Arc::new(Mutex::new(BTreeMap::new()));
    let metadata_generations = Arc::new(Mutex::new(BTreeMap::new()));
    let output_stream = Arc::new(Mutex::new(stream.try_clone().map_err(|error| {
        X11SetupSocketError::new(format!("failed to clone X11 output socket: {error}"))
    })?));
    let output_control_pending = Arc::new(AtomicUsize::new(0));
    let protocol_routing = client_routing.clone();
    let (route_registration, input_receiver, control_channels, protocol_receiver) =
        if let Some(routing) = client_routing {
            if let Err(error) = routing.bind_runtime(&state.runtime) {
                let _ = state.release_client(client);
                return Err(error);
            }
            let admission = admission_lease.as_ref().map(|lease| lease.context());
            let (registration, channels) = match routing
                .register_client_with_admission(client, admission)
            {
                Ok(registration) => registration,
                Err(error) => {
                    let _ = state.release_client(client);
                    return Err(X11SetupSocketError::new(format!(
                        "failed to register X11 client route: {error}"
                    )));
                }
            };
            routing.input_recovery.attach(client, stream.try_clone().map_err(|error|
                X11SetupSocketError::new(format!("failed to clone recovery socket: {error}")))?)
                .map_err(|error| X11SetupSocketError::new(error.to_string()))?;
            (
                Some(registration),
                Some(X11InputEventReceiver::Routed {
                    receiver: channels.input,
                    deliveries: routing.input_delivery_sender.clone(),
                    recovery: Some(routing.input_recovery.clone()),
                }),
                Some(X11ControlChannels::ClientBound {
                    receiver: channels.control,
                    acknowledgements: routing.acknowledgement_sender.clone(),
                    // Present exactly when this registry belongs to a private
                    // instance. Without it a writer holds a registration token
                    // and has nowhere to report its outcome.
                    completion: routing.control_completion(),
                }),
                Some(channels.protocol),
            )
        } else {
            (None, input_receiver, control_channels, None)
        };
    let mut last_published_observation = None;
    let standalone_query_authority = if protocol_routing.is_none() {
        Some(state.runtime.lock()
            .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
            .shared_input_authority())
    } else { None };
    state.runtime.lock()
        .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
        .input_authority_mut().register_query_client(namespace, client.raw());
    let input_writer = input_receiver
        .map(|receiver| {
            spawn_x11_input_event_writer(
                X11InputWriterState {
                    stream: output_stream.clone(),
                    output_control_pending: output_control_pending.clone(),
                    byte_order: setup.byte_order,
                    sequence: event_sequence.clone(),
                    focused_surface_window: focused_surface_window.clone(),
                    core_event_selections: core_event_selections.clone(),
                    xkb_state_details: xkb_state_details.clone(),
                    xkb_modifiers: xkb_modifiers.clone(),
                    surface_windows: surface_windows.clone(),
                    input_authority: protocol_routing
                        .as_ref()
                        .map(|routing| routing.input_authority.clone()),
                    standalone_query_authority,
                    namespace,
                    client,
                },
                receiver,
            )
        })
        .transpose()?;
    let control_writer = control_channels
        .map(|channels| {
            spawn_x11_control_writer(
                output_stream.clone(),
                output_control_pending.clone(),
                setup.byte_order,
                event_sequence.clone(),
                focused_surface_window.clone(),
                surface_windows.clone(),
                metadata_rules.clone(),
                metadata_generations.clone(),
                core_event_selections.clone(),
                xkb_modifiers.clone(),
                state.atoms.clone(),
                state.properties.clone(),
                state.runtime.clone(),
                state.control_runtime_pending.clone(),
                resource_id_range,
                namespace,
                client,
                protocol_routing.clone(),
                channels,
            )
        })
        .transpose()?;
    let protocol_writer = protocol_receiver
        .map(|receiver| {
            spawn_x11_protocol_event_writer(
                output_stream.clone(),
                output_control_pending.clone(),
                setup.byte_order,
                event_sequence.clone(),
                client,
                receiver,
            )
        })
        .transpose()?;
    state.register_client(client_lease)?;
    if let Some((worker_id, sender)) = worker_admission
        && let Some(lease) = admission_lease.as_ref()
    {
        let _ = sender.send(X11CoreClientWorkerAdmission {
            worker_id,
            admission: lease.context().client_id,
        });
    }
    let client_admission = admission_lease.as_ref().map(|lease| lease.context());

    let mut pending_observation = None::<X11DispatchObservation>;
    let mut dispatch_started = false;
    let mut dispatch_complete = false;
    let result = (|| {
        // SCM_RIGHTS on a Unix stream is an in-band barrier, but recvmsg can
        // return the descriptors alongside bytes that precede the request
        // which consumes them. Retain those descriptors until the decoded X11
        // request declares its FD arity instead of binding them to the first
        // header returned by recvmsg.
        let mut pending_request_fds = Vec::new();
        // Created on the first block rather than per connection: most clients
        // never wait behind another's server grab, and an eventfd each would
        // be a descriptor per client for a case that rarely arises.
        let mut grab_wait_notifier: Option<ConnectionNotifier> = None;
        while let Some(received) = read_x11_core_request(stream, setup.byte_order)? {
            let major_opcode = received.major_opcode;
            let request = received.bytes;
            let request_minor_code = if major_opcode >= 128 {
                u16::from(request[1])
            } else {
                0
            };
            let ancillary_fds = received.fds;
            let mut received_fds = Vec::new();
            loop {
                let holder_present = {
                    let runtime = lock_x11_request_runtime(
                        &state.runtime,
                        &state.control_runtime_pending,
                    )?;
                    let authority = runtime.input_authority_mut();
                    authority
                        .server_owner(namespace)
                        .is_some_and(|owner| owner != client.raw())
                };
                if !holder_present {
                    break;
                }
                // Only now is a wake source worth its descriptor. Creating it
                // before the check above would have meant an eventfd per
                // client for a case most clients never reach, and creating it
                // under the guard would put a syscall inside the lock.
                if grab_wait_notifier.is_none() {
                    grab_wait_notifier = Some(ConnectionNotifier::new().map_err(|error| {
                        X11SetupSocketError::new(format!(
                            "failed to create a server-grab wait notifier: {error}"
                        ))
                    })?);
                }
                let notifier = grab_wait_notifier
                    .as_ref()
                    .expect("the notifier was just created");
                let blocked = {
                    let runtime = lock_x11_request_runtime(
                        &state.runtime,
                        &state.control_runtime_pending,
                    )?;
                    let mut authority = runtime.input_authority_mut();
                    if authority
                        .server_owner(namespace)
                        .is_none_or(|owner| owner == client.raw())
                    {
                        // Released while the notifier was being made.
                        false
                    } else {
                        // Registered under the same guard that read the owner.
                        // Registering after releasing it would let a release
                        // in the gap wake nobody, parking this connection
                        // until some unrelated notification arrived.
                        authority.await_server_grab(namespace, notifier);
                        true
                    }
                };
                if !blocked {
                    break;
                }
                // No deadline: the wait ends when the grab is released, the
                // epoch is revoked, the holder disconnects, or this peer
                // departs. A backstop timer here would convert a missing wake
                // into a slow poll, hiding the defect instead of failing on it.
                // A poll failure over descriptors this server owns is a
                // local fault, not peer behaviour, so it keeps the
                // unclassified constructor and reaches the reaper as the
                // server problem it is.
                let wake = ConnectionWait::new(stream.as_fd(), notifier)
                    .wait_until(None)
                    .map_err(|error| {
                        X11SetupSocketError::new(format!(
                            "failed to wait for the server grab to be released: {error}"
                        ))
                    })?;
                match wake {
                    ConnectionWake::Notified | ConnectionWake::Deadline => continue,
                    // The peer is gone. Its remaining requests are moot, and
                    // dispatch ends the same way an ordinary EOF ends it.
                    ConnectionWake::Departed => return Ok(()),
                }
            }
            sequence = sequence.wrapping_add(1);
            let transaction = state.allocate_transaction()?;
            let dispatch_context = XDispatchContext {
                byte_order: setup.byte_order,
                namespace,
                transaction,
                sequence,
                major_opcode,
                client_id: client.raw(),
            };
            dispatch_started = false;
            dispatch_complete = false;
            pending_observation = Some(X11DispatchObservation {
                transaction,
                client,
                admission: client_admission,
                resource_id_range,
                sequence,
                major_opcode,
                minor_opcode: request_minor_code,
                request_stage: X11ObservedRequestStage::Other,
                failure: None,
                result: XDispatchResult {
                    response: None,
                    outputs: Vec::new(),
                    metadata_candidates: Vec::new(),
                },
                surface_routes: Vec::new(),
                surface_output_reservations: Vec::new(),
                cpu_buffer_updates: Vec::new(),
                received_fd_count: 0,
                received_fds: Vec::new(),
                dri3_pixmap_import: None,
                dri3_fence_import: None,
                present_submission: None,
                software_present_submission: None,
                released_dma_bufs: Vec::new(),
                released_fences: Vec::new(),
                server_reply_fd_count: 0,
            });
            let mut pending_msc_deliveries = Vec::new();
            let mut pending_metadata_candidate = None;
            let mut explicit_pointer_completion = None;
            let mut explicit_pointer_release_completion = None;
            let mut parse_failed = false;
            let mut request_stage = X11ObservedRequestStage::Other;
            let mut pixmap_publication_prefix = Vec::new();
            let mut pixmap_prefix_refused = false;
            let (
                mut output,
                cpu_buffer_updates,
                dri3_pixmap_import,
                dri3_fence_import,
                present_submission,
                software_present_submission,
                mut released_dma_bufs,
                released_fences,
                mut server_reply_fds,
                surface_output_reservations,
                surface_routes,
                present_configure,
            ) = match decode_x11_core_request(
                XWireClientContext {
                    byte_order: setup.byte_order,
                    namespace,
                    transaction,
                    resource_id_range: Some(resource_id_range),
                },
                &request,
            ) {
                Ok(mut request) => {
                    let create_surface_route = if let crate::XWireRequest::CreateWindow {
                        packet:
                            crate::XAuthorityRequestPacket {
                                kind:
                                    crate::XAuthorityRequestKind::CreateWindow {
                                        window, surface, ..
                                    },
                                ..
                            },
                        ..
                    } = &mut request
                    {
                        let candidate = surface_generations.candidate(surface.index())?;
                        *surface = candidate;
                        Some((*window, candidate))
                    } else {
                        None
                    };
                    // CurrentTime asks the server to choose the moment, so it
                    // is chosen here, before the selection state stores it.
                    // Resolving later, at the event, would leave the recorded
                    // ownership time zero, and the destroy and client-close
                    // events that report when ownership began would have
                    // nothing to report.
                    if let crate::XWireRequest::Authority(crate::XAuthorityRequestPacket {
                        kind:
                            crate::XAuthorityRequestKind::SetSelectionOwner {
                                timestamp,
                                selection_timestamp,
                                ..
                            },
                        ..
                    }) = &mut request
                    {
                        if *timestamp == 0 {
                            *timestamp = x11_server_time_msec();
                        }
                        if *selection_timestamp == 0 {
                            *selection_timestamp = *timestamp;
                        }
                    }
                    let required_fd_count = request.required_fd_count();
                    pending_request_fds.extend(ancillary_fds);
                    const MAX_PENDING_REQUEST_FDS: usize = sophia_protocol::DMA_BUF_MAX_PLANES * 16;
                    if pending_request_fds.len() > MAX_PENDING_REQUEST_FDS {
                        return Err(X11SetupSocketError::new(
                            "X11 request stream carried too many pending file descriptors",
                        ));
                    }
                    if required_fd_count != 0 {
                        let take = required_fd_count.min(pending_request_fds.len());
                        received_fds.extend(pending_request_fds.drain(..take));
                    }
                    if required_fd_count != received_fds.len() {
                        return Err(X11SetupSocketError::new(format!(
                            "X11 request opcode {major_opcode} required {} file descriptors but received {}",
                            required_fd_count,
                            received_fds.len()
                        )));
                    }
                    // A zero declared size leaves the size to the descriptor,
                    // which is what Chromium's VA-API exports send. Resolving it
                    // before the pure dispatch keeps every bound there applied to
                    // a real size; an unanswerable descriptor keeps the zero.
                    if let crate::XWireRequest::Dri3PixmapFromBuffer { size_bytes, .. } =
                        &mut request
                        && *size_bytes == 0
                        && let Some(fd) = received_fds.first()
                        && let Some(size) = dri3_buffer_size_from_descriptor(fd)
                    {
                        *size_bytes = size;
                    }
                    let event_selection = x11_core_event_selection_update(&request);
                    let xid_request = match &request {
                        crate::XWireRequest::XCMiscGetXIDRange => Some(1u32),
                        crate::XWireRequest::XCMiscGetXIDList { count } => Some(*count),
                        _ => None,
                    };
                    let shm_attach_fd = match &request {
                        crate::XWireRequest::ShmAttachFd {
                            segment,
                            read_only,
                        } => Some((*segment, *read_only)),
                        _ => None,
                    };
                    let shm_created_segment = match &request {
                        crate::XWireRequest::ShmCreateSegment { segment, .. } => Some(*segment),
                        _ => None,
                    };
                    let dri3_recovered_pixmap = match &request {
                        crate::XWireRequest::Dri3BufferFromPixmap { pixmap }
                        | crate::XWireRequest::Dri3BuffersFromPixmap { pixmap } => Some(*pixmap),
                        _ => None,
                    };
                    let dri3_query = matches!(
                        &request,
                        crate::XWireRequest::QueryExtension { name }
                            if name == crate::X_DRI3_EXTENSION_NAME
                    );
                    let dri3_pixmap = match &request {
                        crate::XWireRequest::Dri3PixmapFromBuffer { pixmap, .. }
                        | crate::XWireRequest::Dri3PixmapFromBuffers { pixmap, .. } => {
                            Some(*pixmap)
                        }
                        _ => None,
                    };
                    // Explicit modifiers keep opaque plane geometry, so no row
                    // arithmetic bounds an auxiliary plane. The received
                    // descriptors do, and are read before the pure dispatch:
                    // before this request allocates or publishes anything.
                    let dri3_plane_offset_refused = match &request {
                        crate::XWireRequest::Dri3PixmapFromBuffers {
                            num_buffers,
                            offsets,
                            modifier,
                            ..
                        } if dri3_modifier_is_opaque(*modifier) => {
                            let planes = usize::from(*num_buffers)
                                .min(received_fds.len())
                                .min(sophia_protocol::DMA_BUF_MAX_PLANES);
                            dri3_plane_offset_outside_descriptor(
                                &received_fds[..planes],
                                &offsets[..planes],
                            )
                        }
                        _ => None,
                    };
                    let dri3_fence_request = match &request {
                        crate::XWireRequest::Dri3FenceFromFd {
                            fence,
                            initially_triggered,
                            ..
                        } => Some((*fence, *initially_triggered)),
                        _ => None,
                    };
                    let destroyed_fence = match &request {
                        crate::XWireRequest::SyncDestroyFence { fence } => Some(*fence),
                        _ => None,
                    };
                    let hierarchy_create = match &request {
                        crate::XWireRequest::CreateWindow { packet, parent, .. } => {
                            match &packet.kind {
                                crate::XAuthorityRequestKind::CreateWindow {
                                    window,
                                    geometry,
                                    ..
                                } => {
                                    Some((*window, *parent, *geometry))
                                }
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    let hierarchy_reparent = match &request {
                        crate::XWireRequest::ReparentWindow {
                            window,
                            parent,
                            x,
                            y,
                        } => Some((*window, *parent, *x, *y)),
                        _ => None,
                    };
                    let hierarchy_restack = match &request {
                        crate::XWireRequest::ConfigureWindow {
                            window,
                            sibling,
                            stack_mode,
                            ..
                        } => Some((*window, *sibling, *stack_mode)),
                        _ => None,
                    };
                    let hierarchy_geometry = match &request {
                        crate::XWireRequest::ConfigureWindow {
                            window,
                            x,
                            y,
                            width,
                            height,
                            ..
                        } => Some((*window, *x, *y, *width, *height)),
                        _ => None,
                    };
                    let randr_selection = match &request {
                        crate::XWireRequest::RandrSelectInput { window, enable } => {
                            Some((*window, *enable))
                        }
                        _ => None,
                    };
                    let present_selection = match &request {
                        crate::XWireRequest::PresentSelectInput {
                            event_id,
                            window,
                            event_mask,
                        } => Some((*event_id, *window, *event_mask)),
                        _ => None,
                    };
                    let present_msc_notify = match &request {
                        crate::XWireRequest::PresentNotifyMsc {
                            window,
                            serial,
                            target_msc,
                            ..
                        } => Some((*window, *serial, *target_msc)),
                        _ => None,
                    };
                    let pending_present = match &request {
                        crate::XWireRequest::PresentPixmap {
                            window,
                            pixmap,
                            serial,
                            idle_fence,
                            options,
                            ..
                        } => Some((*window, *pixmap, *serial, *idle_fence, options & 0x0a == 0x08)),
                        _ => None,
                    };
                    let present_request = match &request {
                        crate::XWireRequest::PresentPixmap {
                            window,
                            wait_fence,
                            idle_fence,
                            x_offset,
                            y_offset,
                            ..
                        } => Some((*window, *wait_fence, *idle_fence, *x_offset, *y_offset)),
                        _ => None,
                    };
                    let xkb_selection = match &request {
                        crate::XWireRequest::XkbSelectEvents {
                            affect_which,
                            clear,
                            select_all,
                            state_details,
                        } => Some((*affect_which, *clear, *select_all, *state_details)),
                        _ => None,
                    };
                    let xkb_get_state = matches!(request, crate::XWireRequest::XkbGetState);
                    let xfixes_selection_input = match &request {
                        crate::XWireRequest::XfixesSelectSelectionInput {
                            window,
                            selection,
                            event_mask,
                        } => Some((*window, *selection, *event_mask)),
                        _ => None,
                    };
                    // Ownership changes carry the cause with them, so the
                    // subtype a watcher receives comes from the request that
                    // caused it rather than from comparing before and after.
                    let selection_owner_change = match &request {
                        crate::XWireRequest::Authority(packet) => match &packet.kind {
                            crate::XAuthorityRequestKind::SetSelectionOwner {
                                selection,
                                owner,
                                timestamp,
                                selection_timestamp,
                                kind,
                            } => Some((
                                *selection,
                                *owner,
                                *timestamp,
                                *selection_timestamp,
                                *kind,
                            )),
                            _ => None,
                        },
                        _ => None,
                    };
                    let selection_property_read = selection_property_read_trace(&request);
                    let requested_input_focus = match &request {
                        crate::XWireRequest::SetInputFocus { focus, .. } => Some(*focus),
                        _ => None,
                    };
                    let mapped_window = match &request {
                        crate::XWireRequest::Authority(crate::XAuthorityRequestPacket {
                            kind: crate::XAuthorityRequestKind::MapWindow { window, .. },
                            ..
                        }) => Some(*window),
                        _ => None,
                    };
                    let mapped_subwindows = matches!(&request, crate::XWireRequest::MapSubwindows { .. });
                    let unmapped_window = match &request {
                        crate::XWireRequest::UnmapWindow { window } => Some(*window),
                        _ => None,
                    };
                    let output_reservation_property = match &request {
                        crate::XWireRequest::ChangeProperty(change) => {
                            Some((change.window, change.property))
                        }
                        crate::XWireRequest::DeleteProperty { window, property } => {
                            Some((*window, *property))
                        }
                        _ => None,
                    };
                    let metadata_property_update = match &request {
                        crate::XWireRequest::ChangeProperty(change) => {
                            Some((change.window, change.property))
                        }
                        crate::XWireRequest::DeleteProperty { window, property } => {
                            Some((*window, *property))
                        }
                        _ => None,
                    };
                    let output_reservation_surface =
                        if let Some((window, property)) = output_reservation_property {
                            surface_windows
                                .lock()
                                .map_err(|_| {
                                    X11SetupSocketError::new(
                                        "X11 surface/window map lock poisoned",
                                    )
                                })?
                                .iter()
                                .find_map(|(surface, candidate)| {
                                    (*candidate == window).then_some((*surface, window, property))
                                })
                        } else {
                            None
                        };
                    request_stage = x11_observed_request_stage(&request);
                    // A refusal here must stay a refusal of one request. Ending
                    // the reader instead leaves the socket half-open -- the
                    // writer side keeps it alive, the client sees no EOF, no
                    // error, and no further reply ever -- which reads as a
                    // client that silently stopped drawing. One request was
                    // wrong; the conversation is not over.
                    let mut present_queue_refused = None;
                    let queued_present = if let Some((window, pixmap, serial, idle_fence, suboptimal)) =
                        pending_present
                        && let Some(routing) = protocol_routing.as_ref()
                    {
                        match routing.queue_present(
                            transaction,
                            client,
                            window,
                            pixmap,
                            serial,
                            idle_fence,
                            suboptimal,
                        ) {
                            Ok(()) => true,
                            Err(error) => {
                                tracing::warn!(
                                    "sophia_x11_present_refused schema=1 client={} window={:#x} error={error}",
                                    client.raw(),
                                    window.local.raw(),
                                );
                                present_queue_refused = Some(window);
                                false
                            }
                        }
                    } else {
                        false
                    };
                    let explicit_pointer_preparation = x11_prepare_explicit_pointer_grab(
                        state,
                        protocol_routing.as_ref(),
                        client_admission,
                        namespace,
                        client,
                        &request,
                        last_published_observation,
                    )?;
                    let explicit_pointer_release = x11_begin_explicit_pointer_release(
                        state,
                        protocol_routing.as_ref(),
                        client_admission,
                        namespace,
                        client,
                        &request,
                    )?;
                    let prepared_pixmap_export = match dri3_recovered_pixmap {
                        Some(drawable) => state.prepare_exported_pixmap(namespace, drawable)?,
                        None => None,
                    };
                    let mut runtime = lock_x11_request_runtime(
                        &state.runtime,
                        &state.control_runtime_pending,
                    )?;
                    let mut atoms = state
                        .atoms
                        .lock()
                        .map_err(|_| X11SetupSocketError::new("X11 atom table lock poisoned"))?;
                    let mut properties = state.properties.lock().map_err(|_| {
                        X11SetupSocketError::new("X11 property table lock poisoned")
                    })?;
                    dispatch_started = true;
                    let released_fence = destroyed_fence
                        .and_then(|fence| runtime.dri3_fence_handle(namespace, fence).ok());
                    let configured_geometry_before = hierarchy_geometry.and_then(
                        |(window, _, _, _, _)| {
                            runtime.window_geometry(namespace, window).ok()
                        },
                    );
                    let mut explicit_pointer_preparation = explicit_pointer_preparation;
                    if let X11ExplicitPointerGrabPreparation::Prepared {
                        identity,
                        anchor,
                        replaces,
                    } = explicit_pointer_preparation
                    {
                        let current = runtime.input_authority_mut().pointer_grab(namespace);
                        let same_grab = current.is_none_or(|grab| grab.owner == client.raw())
                            && current.and_then(|grab| grab.route_lease) == replaces;
                        let same_surface = match anchor {
                            crate::XAuthorityExplicitPointerGrabAnchor::AdmissionDefault => true,
                            crate::XAuthorityExplicitPointerGrabAnchor::Surface(surface) => {
                                let window = match &request {
                                    crate::XWireRequest::GrabPointer { window, .. }
                                    | crate::XWireRequest::XiGrabDevice { window, .. } => *window,
                                    _ => unreachable!("only grab requests prepare leases"),
                                };
                                runtime
                                    .window_presentation_root_and_offset(namespace, window)
                                    .is_ok_and(|(_, current, _, _)| current == surface)
                                    && runtime
                                        .window_map_state(namespace, window)
                                        .is_ok_and(|state| state == crate::XMapState::Viewable)
                            }
                        };
                        let same_epoch = protocol_routing.as_ref().is_some_and(|routing| {
                            routing.input_control_epoch.load(Ordering::Acquire) == identity.control_epoch
                        });
                        if !same_grab || !same_surface || !same_epoch {
                            explicit_pointer_completion = Some((identity, false, anchor));
                            explicit_pointer_preparation =
                                X11ExplicitPointerGrabPreparation::Rejected(if same_surface { 1 } else { 3 });
                        }
                    }
                    let release_is_stale = explicit_pointer_release.is_some_and(|identity| {
                        runtime
                            .input_authority_mut()
                            .pointer_grab(namespace)
                            .is_none_or(|grab| grab.owner != client.raw() || grab.route_lease != Some(identity))
                    });
                    let pixmap_export_changed = prepared_pixmap_export.is_some_and(|token| {
                        let expected = runtime.pixmap_export_buffers(token).ok();
                        let current = dri3_recovered_pixmap.and_then(|drawable| {
                            runtime.dri3_pixmap_buffers(namespace, drawable).ok()
                        });
                        !expected.zip(current).is_some_and(|(expected, current)| {
                            expected.0.handle == current.0.handle
                        })
                    });
                    let mut output = match explicit_pointer_preparation {
                        _ if pixmap_export_changed => {
                            runtime.begin_dispatch();
                            XDispatchResult {
                                response: None,
                                outputs: vec![crate::XClientOutput::Error(crate::XClientError {
                                    code: crate::XErrorCode::BadPixmap,
                                    sequence,
                                    resource_id: dri3_recovered_pixmap.map_or(0, |id| id.local.raw() as u32),
                                    minor_code: request_minor_code,
                                    major_code: major_opcode,
                                })],
                                metadata_candidates: Vec::new(),
                            }
                        }
                        _ if dri3_plane_offset_refused.is_some() => {
                            let offset =
                                dri3_plane_offset_refused.expect("guarded by the match arm");
                            runtime.begin_dispatch();
                            XDispatchResult {
                                response: None,
                                outputs: vec![crate::XClientOutput::Error(crate::XClientError {
                                    code: crate::XErrorCode::BadValue,
                                    sequence: dispatch_context.sequence,
                                    resource_id: offset,
                                    minor_code: u16::from(
                                        crate::X_DRI3_PIXMAP_FROM_BUFFERS_MINOR_OPCODE,
                                    ),
                                    major_code: crate::X_DRI3_MAJOR_OPCODE,
                                })],
                                metadata_candidates: Vec::new(),
                            }
                        }
                        _ if release_is_stale => {
                            runtime.begin_dispatch();
                            XDispatchResult {
                                response: None,
                                outputs: Vec::new(),
                                metadata_candidates: Vec::new(),
                            }
                        }
                        X11ExplicitPointerGrabPreparation::Rejected(status) => {
                            runtime.begin_dispatch();
                            XDispatchResult {
                                response: None,
                                outputs: vec![crate::XClientOutput::Reply(
                                    crate::XClientReply::GrabStatus {
                                        sequence: dispatch_context.sequence,
                                        status,
                                    },
                                )],
                                metadata_candidates: Vec::new(),
                            }
                        }
                        _ if present_queue_refused.is_some() => {
                            let window = present_queue_refused.expect("guarded by the match arm");
                            runtime.begin_dispatch();
                            XDispatchResult {
                                response: None,
                                outputs: vec![crate::XClientOutput::Error(crate::XClientError {
                                    code: crate::XErrorCode::BadWindow,
                                    sequence: dispatch_context.sequence,
                                    resource_id: u32::try_from(window.local.raw()).unwrap_or(0),
                                    minor_code: u16::from(crate::X_PRESENT_PIXMAP_MINOR_OPCODE),
                                    major_code: crate::X_PRESENT_MAJOR_OPCODE,
                                })],
                                metadata_candidates: Vec::new(),
                            }
                        }
                        _ => {
                            dispatch_started = true;
                            dispatch_x11_wire_request(dispatch_context, request, &mut runtime, &mut atoms, &mut properties)
                        },
                    };
                    if let X11ExplicitPointerGrabPreparation::Prepared {
                        identity, anchor, ..
                    } = explicit_pointer_preparation
                    {
                        let local_admitted = output.outputs.iter().any(|output| {
                            matches!(
                                output,
                                crate::XClientOutput::Reply(crate::XClientReply::GrabStatus { status: 0, .. })
                            )
                        });
                        let attached = local_admitted
                            && runtime
                                .input_authority_mut()
                                .set_pointer_route_lease(namespace, client.raw(), identity)
                                .is_ok();
                        explicit_pointer_completion = Some((identity, attached, anchor));
                    }
                    explicit_pointer_release_completion = explicit_pointer_release;
                    let mapped_windows = if mapped_subwindows {
                        output
                            .response
                            .as_ref()
                            .into_iter()
                            .flat_map(|response| response.surfaces.iter())
                            .filter_map(|surface| {
                                let window = XResourceId { local: surface.local_id };
                                runtime
                                    .window_map_state(namespace, window)
                                    .ok()
                                    .filter(|state| *state != crate::XMapState::Unmapped)
                                    .map(|_| window)
                            })
                            .collect::<Vec<_>>()
                    } else {
                        mapped_window
                            .filter(|window| {
                                runtime
                                    .window_map_state(namespace, *window)
                                    .is_ok_and(|state| state != crate::XMapState::Unmapped)
                            })
                            .into_iter()
                            .collect()
                    };

                    // Selection changes are told here, where this client's own
                    // outputs are still open. A watcher that is also the client
                    // that caused the change must receive the event against
                    // this request's sequence, and appending to its outputs is
                    // what orders it correctly; routing it asynchronously would
                    // stamp whatever sequence had last been written.
                    if let Some(routing) = protocol_routing.as_ref() {
                        let mut changes = Vec::new();
                        if let Some((selection, owner, time, selection_time, kind)) =
                            selection_owner_change
                            && output
                                .outputs
                                .iter()
                                .all(|out| !matches!(out, crate::XClientOutput::Error(_)))
                        {
                            changes.push((
                                namespace,
                                selection_change_subtype(kind),
                                selection,
                                owner.unwrap_or(crate::XResourceId::NONE),
                                time,
                                selection_time,
                            ));
                        }
                        // Ownerships ended because a window or a client went
                        // away. The event time is now; the selection time stays
                        // the one the ownership began with, which is what the
                        // watcher is being told about.
                        // Each retired ownership carries the namespace it
                        // belonged to. The queue is shared, so a drain here may
                        // pick up another namespace's entries; routing them
                        // under this connection's namespace would deliver them
                        // to the wrong watchers, or to none.
                        for retired in runtime.take_retired_selection_ownerships() {
                            let Some(owner_namespace) = retired.current.namespace else {
                                // Unattributable, so undeliverable: there is no
                                // namespace whose watchers this belongs to.
                                continue;
                            };
                            changes.push((
                                owner_namespace,
                                selection_change_subtype(retired.kind),
                                retired.current.selection,
                                crate::XResourceId::NONE,
                                x11_server_time_msec(),
                                retired.current.selection_timestamp,
                            ));
                        }
                        for (owner_namespace, subtype, selection, owner, time, selection_time) in
                            changes
                        {
                            for (recipient, window) in routing
                                .xfixes_selection_subscribers(owner_namespace, selection, subtype)
                                .map_err(|error| {
                                    X11SetupSocketError::new(format!(
                                        "failed to inspect XFixes selection subscriptions: {error}"
                                    ))
                                })?
                            {
                                let event = crate::XClientEvent::XfixesSelectionNotify {
                                    sequence,
                                    subtype,
                                    window,
                                    owner,
                                    selection,
                                    time,
                                    selection_time,
                                };
                                if recipient == client {
                                    output.outputs.push(crate::XClientOutput::Event(event));
                                } else if let Err(error) =
                                    routing.route_protocol(recipient, event)
                                {
                                    if let XServerFrontendRouteError::ClientQueueFull {
                                        client: stalled,
                                    } = error
                                    {
                                        routing
                                            .disconnect_saturated_recipient(stalled)
                                            .map_err(|error| {
                                                X11SetupSocketError::new(format!(
                                                    "failed to end a stalled XFixes watcher: {error}"
                                                ))
                                            })?;
                                    } else if !x11_recipient_is_gone(&error) {
                                        return Err(X11SetupSocketError::new(format!(
                                            "failed to route an XFixes selection change: {error}"
                                        )));
                                    }
                                }
                            }
                        }
                    }
                    trace_selection_property_read_result(selection_property_read, &output);
                    // DRI3 presence depends on a render-device provider, which
                    // the pure dispatch cannot see. Both the query and the
                    // enumeration are corrected here, from the one place that
                    // knows: a client that enumerates and then queries must not
                    // be told two different things about the same extension.
                    if !state.has_render_device_provider() {
                        for client_output in &mut output.outputs {
                            match client_output {
                                crate::XClientOutput::Reply(
                                    crate::XClientReply::QueryExtension {
                                        present,
                                        major_opcode,
                                        first_event,
                                        first_error,
                                        ..
                                    },
                                ) if dri3_query => {
                                    *present = false;
                                    *major_opcode = 0;
                                    *first_event = 0;
                                    *first_error = 0;
                                }
                                crate::XClientOutput::Reply(
                                    crate::XClientReply::ListExtensions { names, .. },
                                ) => {
                                    names.retain(|name| name != crate::X_DRI3_EXTENSION_NAME);
                                }
                                _ => {}
                            }
                        }
                    }
                    if xkb_get_state {
                        for client_output in &mut output.outputs {
                            if let crate::XClientOutput::Reply(crate::XClientReply::XkbGetState {
                                modifiers,
                                ..
                            }) = client_output
                            {
                                *modifiers = xkb_modifiers.load(Ordering::Acquire) as u8;
                            }
                        }
                    }
                    if std::env::var_os("SOPHIA_LIVE_SESSION_DIAGNOSTIC").is_some()
                        && request_stage == X11ObservedRequestStage::KeyboardMapping
                    {
                        tracing::debug!(
                            "sophia_x11_keyboard_map schema=1 status=served detail_redacted=true"
                        );
                    }
                    let dispatch_succeeded = !output
                        .outputs
                        .iter()
                        .any(|output| matches!(output, crate::XClientOutput::Error(_)));
                    let removed_surface_routes = if dispatch_succeeded { {
                            output
                                .response
                                .as_ref()
                                .map(|response| response.removed_surfaces.clone())
                                .unwrap_or_default()
                        } } else { Default::default() };
                    let present_configure = dispatch_succeeded
                        .then_some(hierarchy_geometry)
                        .flatten()
                        .and_then(|(window, _, _, _, _)| {
                            let geometry = runtime.window_geometry(namespace, window).ok()?;
                            (configured_geometry_before != Some(geometry))
                                .then_some((window, geometry))
                        });
                    let hierarchy_geometry = hierarchy_geometry.and_then(
                        |(window, _, _, _, _)| {
                            runtime
                                .window_geometry(namespace, window)
                                .ok()
                                .map(|geometry| {
                                    (
                                        window,
                                        Some(crate::dispatch::clamp_i16(geometry.x)),
                                        Some(crate::dispatch::clamp_i16(geometry.y)),
                                        Some(crate::dispatch::clamp_u16(geometry.width)),
                                        Some(crate::dispatch::clamp_u16(geometry.height)),
                                    )
                                })
                        },
                    );
                    if dispatch_succeeded {
                        if !removed_surface_routes.is_empty() {
                            {
                                let mut windows = surface_windows.lock().map_err(|_| {
                                    X11SetupSocketError::new("X11 surface/window map lock poisoned")
                                })?;
                                for surface in &removed_surface_routes {
                                    windows.remove(surface);
                                }
                            }
                            {
                                let mut rules = metadata_rules.lock().map_err(|_| {
                                    X11SetupSocketError::new("X11 metadata rule lock poisoned")
                                })?;
                                let mut generations =
                                    metadata_generations.lock().map_err(|_| {
                                        X11SetupSocketError::new(
                                            "X11 metadata generation lock poisoned",
                                        )
                                    })?;
                                for surface in &removed_surface_routes {
                                    rules.remove(surface);
                                    generations.remove(surface);
                                }
                            }
                            if let Some(routing) = protocol_routing.as_ref() {
                                for surface in &removed_surface_routes {
                                    routing.remove_surface(*surface).map_err(|error| {
                                        X11SetupSocketError::new(format!(
                                            "failed to retire X11 surface route: {error}"
                                        ))
                                    })?;
                                }
                            }
                        }
                        if let Some((window, surface)) = create_surface_route
                            && output.response.as_ref().is_some_and(|response| {
                                response.outcome == crate::XAuthorityResponseOutcome::Accepted
                            })
                        {
                            surface_generations.admit(surface)?;
                            surface_windows
                                .lock()
                                .map_err(|_| {
                                    X11SetupSocketError::new("X11 surface/window map lock poisoned")
                                })?
                                .insert(surface, window);
                            if let Some(routing) = protocol_routing.as_ref() {
                                routing
                                    .register_surface(client, namespace, surface, window)
                                    .map_err(|error| {
                                        X11SetupSocketError::new(format!(
                                            "failed to register X11 surface route: {error}"
                                        ))
                                    })?;
                            }
                        }
                        if let Some(focus) = requested_input_focus {
                            focused_surface_window.store(focus.local.raw(), Ordering::Release);
                        }
                        let mut selections = core_event_selections.lock().map_err(|_| {
                            X11SetupSocketError::new("X11 core event selection lock poisoned")
                        })?;
                        if let Some((window, event_mask, do_not_propagate_mask)) = event_selection {
                            selections.update(window, event_mask, do_not_propagate_mask);
                            if let Some(mask) = event_mask
                                && let Some(routing) = protocol_routing.as_ref()
                            {
                                routing.select_core_events(client, window, mask).map_err(
                                    |error| {
                                        X11SetupSocketError::new(format!(
                                            "failed to update core X11 event subscription: {error}"
                                        ))
                                    },
                                )?;
                            }
                        }
                        if let Some((window, parent, geometry)) = hierarchy_create {
                            selections.register(window, parent, geometry);
                            if let Some(routing) = protocol_routing.as_ref() {
                                routing
                                    .register_window_parent(client, window, parent)
                                    .map_err(|error| {
                                        X11SetupSocketError::new(format!(
                                            "failed to register X11 window hierarchy: {error}"
                                        ))
                                    })?;
                            }
                        }
                        if let Some((window, parent, x, y)) = hierarchy_reparent {
                            selections.reparent(window, parent, x, y);
                        }
                        if let Some((window, sibling, mode)) = hierarchy_restack {
                            selections.restack(window, sibling, mode);
                        }
                        if let Some((window, x, y, width, height)) = hierarchy_geometry {
                            selections.configure_geometry(window, x, y, width, height);
                        }
                        for window in mapped_windows {
                            selections.observe_mapped(window);
                        }
                        if let Some(window) = unmapped_window {
                            selections.observe_unmapped(window);
                        }
                        // Routing reads the window's subscriptions and parent to
                        // find who is owed its DestroyNotify, so those entries
                        // have to outlive the window and are cleared after the
                        // notification is routed. Clearing here deleted the
                        // recipients before the event addressed to them was
                        // delivered.
                        // Driven off what was actually destroyed rather than
                        // the request's single window. DestroySubwindows removes
                        // a whole set, and a request-shaped extraction cannot
                        // name them -- selection state would survive for every
                        // child without anything reporting it.
                        for window in output.outputs.iter().filter_map(|entry| match entry {
                            crate::XClientOutput::Event(crate::XClientEvent::DestroyNotify {
                                event,
                                window,
                                ..
                            }) if event == window => Some(*window),
                            _ => None,
                        }) {
                            selections.remove(window);
                        }
                        if let Some((window, selection, mask)) = xfixes_selection_input
                            && let Some(routing) = protocol_routing.as_ref()
                        {
                            routing
                                .select_xfixes_selection_input(client, namespace, window, selection, mask)
                                .map_err(|error| {
                                    X11SetupSocketError::new(format!(
                                        "failed to update XFixes selection subscription: {error}"
                                    ))
                                })?;
                        }
                        if let Some((window, mask)) = randr_selection
                            && let Some(routing) = protocol_routing.as_ref()
                        {
                            routing
                                .select_randr_input(client, window, mask)
                                .map_err(|error| {
                                    X11SetupSocketError::new(format!(
                                        "failed to update RandR subscription: {error}"
                                    ))
                                })?;
                        }
                        if let Some((event_id, window, mask)) = present_selection
                            && let Some(routing) = protocol_routing.as_ref()
                        {
                            if crate::x11_authority_trace_enabled() {
                                // Which drawable a client watches decides whether
                                // it is ever told that drawable's size. A
                                // subscription on one window and a configure on
                                // another look identical from either side alone.
                                tracing::info!(
                                    "sophia_x11_present_select schema=1 status=recorded window={:#x} event_id={:#x} mask={mask:#x}",
                                    window.local.raw(),
                                    event_id.local.raw(),
                                );
                            }
                            routing
                                .select_present_input(client, event_id, window, mask)
                                .map_err(|error| {
                                    X11SetupSocketError::new(format!(
                                        "failed to update Present subscription: {error}"
                                    ))
                                })?;
                        }
                        if let Some((window, serial, target_msc)) = present_msc_notify
                            && let Some(routing) = protocol_routing.as_ref()
                        {
                            // Mesa blocks on the answer, so this runs only after
                            // dispatch validated the window -- an invalid window
                            // gets its error instead, never a stray event.
                            pending_msc_deliveries = routing
                                .prepare_present_msc_notify(window, serial, target_msc)
                                .map_err(|error| {
                                    X11SetupSocketError::new(format!(
                                        "failed to answer Present NotifyMSC: {error}"
                                    ))
                                })?;
                        }
                        if let Some((affect_which, clear, select_all, state)) = xkb_selection {
                            let mut details = xkb_state_details.load(Ordering::Acquire);
                            if clear & 4 != 0 {
                                details = 0;
                            }
                            if select_all & 4 != 0 {
                                details = u16::MAX;
                            }
                            if affect_which & 4 != 0
                                && let Some((affect, selected)) = state
                            {
                                details = (details & !affect) | (selected & affect);
                            }
                            xkb_state_details.store(details, Ordering::Release);
                        }
                    }
                    if queued_present
                        && !dispatch_succeeded
                        && let Some(routing) = protocol_routing.as_ref()
                    {
                        routing.cancel_present(transaction).map_err(|error| {
                            X11SetupSocketError::new(format!(
                                "failed to cancel rejected Present feedback: {error}"
                            ))
                        })?;
                    }
                    // The CPU update belongs to this dispatch. Keep it under
                    // the runtime lock so a simultaneous client cannot take
                    // an update generated by this request.
                    let cpu_buffer_updates = runtime.take_cpu_buffer_updates();
                    let dri3_pixmap_import = dri3_pixmap.and_then(|pixmap| {
                        runtime
                            .dri3_pixmap_descriptor(namespace, pixmap)
                            .ok()
                            .map(|descriptor| XAuthorityDri3PixmapImport { pixmap, descriptor })
                    });
                    // Keep the descriptors this import arrived with. DRI3 lets
                    // a client ask for its own buffer back, and the authority
                    // cannot borrow the renderer's copy to answer -- the import
                    // boundary keeps renderer handles out of protocol
                    // authorities. They are dropped with the pixmap.
                    if let Some(pixmap) = dri3_pixmap
                        && dri3_pixmap_import.is_some()
                    {
                        let retained = received_fds
                            .iter()
                            .map(|fd| fd.try_clone().map(Arc::new))
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(|error| {
                                X11SetupSocketError::new(format!(
                                    "failed to retain DRI3 plane descriptor: {error}"
                                ))
                            })?;
                        runtime
                            .attach_dri3_plane_fds(namespace, pixmap, retained)
                            .map_err(|error| {
                                X11SetupSocketError::new(format!(
                                    "failed to record DRI3 plane descriptors: {error:?}"
                                ))
                            })?;
                    }
                    let dri3_fence_import = dispatch_succeeded
                        .then_some(dri3_fence_request)
                        .flatten()
                        .and_then(|(fence, initially_triggered)| {
                            runtime
                                .dri3_fence_handle(namespace, fence)
                                .ok()
                                .map(|handle| XAuthorityDri3FenceImport {
                                    fence,
                                    handle,
                                    initially_triggered,
                                })
                        });
                    let present_submission = dispatch_succeeded
                        .then_some(present_request)
                        .flatten()
                        .and_then(|(window, wait_fence, idle_fence, x_offset, y_offset)| {
                            let response = output.response.as_ref()?;
                            let transaction = response.transactions.first()?;
                            let sophia_protocol::BufferSource::DmaBuf { handle } =
                                transaction.target_buffer()
                            else {
                                return None;
                            };
                            let (_, surface, child_x, child_y) = runtime
                                .window_presentation_root_and_offset(namespace, window)
                                .ok()?;
                            if transaction.surface != surface {
                                return None;
                            }
                            Some(XAuthorityPresentSubmission {
                                transaction: response.transaction,
                                surface: transaction.surface,
                                buffer: sophia_protocol::BufferHandle::from_raw(handle),
                                x_offset: child_x.saturating_add(i32::from(x_offset)),
                                y_offset: child_y.saturating_add(i32::from(y_offset)),
                                acquire_fence: wait_fence.and_then(|fence| {
                                    runtime.dri3_fence_handle(namespace, fence).ok()
                                }),
                                idle_fence: idle_fence.and_then(|fence| {
                                    runtime.dri3_fence_handle(namespace, fence).ok()
                                }),
                            })
                        });
                    if queued_present
                        && let Some(routing) = protocol_routing.as_ref()
                        && let Some(present) = present_submission.as_ref()
                        && let Some((window, pixmap, _, _, _)) = pending_present
                        && let Some(subject) = runtime.present_allocation_subject(
                            namespace, client.raw(), window, pixmap, present,
                        )
                    {
                        routing.record_present_allocation_subject(subject);
                    }
                    let software_present_submission = dispatch_succeeded
                        .then_some(present_request)
                        .flatten()
                        .and_then(|(_, wait_fence, idle_fence, _, _)| {
                            let response = output.response.as_ref()?;
                            let transaction = response.transactions.first()?;
                            if !matches!(
                                transaction.target_buffer(),
                                sophia_protocol::BufferSource::CpuBuffer { .. }
                            ) {
                                return None;
                            }
                            Some(crate::XAuthoritySoftwarePresentSubmission {
                                transaction: response.transaction,
                                surface: transaction.surface,
                                acquire_fence: wait_fence.and_then(|fence| {
                                    runtime.dri3_fence_handle(namespace, fence).ok()
                                }),
                                idle_fence: idle_fence.and_then(|fence| {
                                    runtime.dri3_fence_handle(namespace, fence).ok()
                                }),
                            })
                        });
                    let mut server_reply_fds = Vec::new();
                    // The reply promised `nfd` descriptors; this is where they
                    // travel. Only on success -- a refused recovery carries an
                    // error, and descriptors attached to it would leave the
                    // client reading a buffer it was never given.
                    if dispatch_succeeded
                        && let Some(pixmap) = dri3_recovered_pixmap
                        && let Ok((_, plane_fds)) = runtime.dri3_pixmap_buffers(namespace, pixmap)
                    {
                        for fd in plane_fds {
                            server_reply_fds.push(fd.try_clone().map_err(|error| {
                                X11SetupSocketError::new(format!(
                                    "failed to hand back DRI3 plane descriptor: {error}"
                                ))
                            })?);
                        }
                    }
                    // The descriptor is the segment's memory, so it is mapped
                    // here rather than in dispatch, which never sees it. A
                    // descriptor that cannot be mapped leaves nothing recorded
                    // and the client is told, instead of holding a segment name
                    // that answers with no memory.
                    if dispatch_succeeded
                        && let Some((segment, read_only)) = shm_attach_fd
                    {
                        let mapped = received_fds
                            .first()
                            .ok_or(sophia_sysv_shm::AccessError::MissingSegment)
                            .and_then(|descriptor| {
                                sophia_sysv_shm::DescriptorMapping::map(
                                    descriptor.as_fd(),
                                    read_only,
                                )
                            });
                        match mapped {
                            Ok(mapping) => runtime
                                .attach_shm_descriptor_segment(
                                    namespace,
                                    segment,
                                    Arc::new(sophia_sysv_shm::ClientMapping::Descriptor(mapping)),
                                    read_only,
                                    u64::from(sequence),
                                )
                                .map_err(|error| {
                                    X11SetupSocketError::new(format!(
                                        "failed to record MIT-SHM segment: {error:?}"
                                    ))
                                })?,
                            Err(_) => {
                                output.outputs =
                                    vec![crate::XClientOutput::Error(crate::XClientError {
                                        code: crate::XErrorCode::BadValue,
                                        sequence,
                                        resource_id: u32::try_from(segment.local.raw())
                                            .unwrap_or(0),
                                        minor_code: u16::from(
                                            crate::X_MIT_SHM_ATTACH_FD_MINOR_OPCODE,
                                        ),
                                        major_code: crate::X_MIT_SHM_MAJOR_OPCODE,
                                    })];
                            }
                        }
                    }
                    if dispatch_succeeded
                        && let Some(segment) = shm_created_segment
                        && let Some(descriptor) = runtime.take_shm_reply_descriptor(segment)
                    {
                        server_reply_fds.push(descriptor);
                    }
                    // Dispatch answered "none available", which is correct
                    // and needs no repair if this layer cannot do better. The
                    // range counter lives here, so this is the only place that
                    // can turn that into a grant.
                    if dispatch_succeeded
                        && let Some(requested) = xid_request
                        && let Some((base, size)) = state.grant_client_resource_range()
                    {
                        for output in &mut output.outputs {
                            match output {
                                crate::XClientOutput::Reply(
                                    crate::XClientReply::XCMiscGetXIDRange {
                                        start_id, count, ..
                                    },
                                ) => {
                                    *start_id = base;
                                    *count = size;
                                }
                                crate::XClientOutput::Reply(
                                    crate::XClientReply::XCMiscGetXIDList { ids, .. },
                                ) => {
                                    // Bounded before it is honoured: the
                                    // request carries a CARD32, and the reply
                                    // is a list this process has to hold.
                                    let wanted = requested
                                        .min(crate::X_XC_MISC_MAX_XID_LIST)
                                        .min(size);
                                    *ids = (0..wanted).map(|offset| base + offset).collect();
                                }
                                _ => {}
                            }
                        }
                    }
                    let surface_output_reservations = dispatch_succeeded
                        .then_some(output_reservation_surface)
                        .flatten()
                        .filter(|(_, _, property)| {
                            matches!(
                                atoms.name(*property),
                                Some(
                                    X_ATOM_NAME_NET_WM_STRUT
                                        | X_ATOM_NAME_NET_WM_STRUT_PARTIAL
                                )
                            )
                        })
                        .map(|(surface, window, _)| SurfaceOutputReservations {
                            surface,
                            reservations: x_output_reservations_for_window(
                                &properties,
                                &atoms,
                                namespace,
                                window,
                                setup.byte_order,
                                Rect {
                                    x: 0,
                                    y: 0,
                                    width: setup_success.root_size.width,
                                    height: setup_success.root_size.height,
                                },
                            ),
                        })
                        .into_iter()
                        .collect();
                    if dispatch_succeeded
                        && let Some((window, property)) = metadata_property_update
                        && atoms
                            .name(property)
                            .is_some_and(crate::is_metadata_candidate_name)
                        && protocol_routing.is_some()
                    {
                        let surface = surface_windows
                            .lock()
                            .map_err(|_| {
                                X11SetupSocketError::new("X11 surface/window map lock poisoned")
                            })?
                            .iter()
                            .find_map(|(surface, candidate)| {
                                (*candidate == window).then_some(*surface)
                            });
                        if let Some(surface) = surface {
                            let rule = metadata_rules
                                .lock()
                                .map_err(|_| {
                                    X11SetupSocketError::new("X11 metadata rule lock poisoned")
                                })?
                                .get(&surface)
                                .copied();
                            if let Some(rule) = rule {
                                let generation = next_x11_metadata_generation(
                                    &metadata_generations,
                                    surface,
                                )?;
                                let mut candidate = crate::reduce_window_metadata(
                                    &properties,
                                    &atoms,
                                    namespace,
                                    window,
                                    surface,
                                    Some(rule),
                                )
                                .unwrap_or(sophia_protocol::ReducedMetadataCandidate {
                                    surface,
                                    label: None,
                                    disclosure: rule.disclosure,
                                    generation,
                                });
                                candidate.generation = generation;
                                pending_metadata_candidate = Some(candidate);
                            }
                        }
                    }
                    let mut changed_surfaces = BTreeSet::new();
                    if dispatch_succeeded
                        && let Some(response) = output.response.as_ref()
                    {
                        changed_surfaces.extend(
                            response
                                .transactions
                                .iter()
                                .map(|transaction| transaction.surface),
                        );
                        changed_surfaces.extend(
                            response.surfaces.iter().map(|surface| surface.surface),
                        );
                        for surface in &response.removed_surfaces {
                            changed_surfaces.remove(surface);
                        }
                    }
                    let surface_routes = if let Some(routing) = protocol_routing.as_ref() {
                        changed_surfaces
                            .into_iter()
                            .map(|surface| {
                                routing
                                    .surface_route_observation(surface)
                                    .map_err(|error| {
                                        X11SetupSocketError::new(format!(
                                            "failed to resolve X11 surface owner route: {error}"
                                        ))
                                    })?
                                    .ok_or_else(|| {
                                        X11SetupSocketError::new(format!(
                                            "accepted X11 surface has no frontend owner route: {surface:?}"
                                        ))
                                    })
                            })
                            .collect::<Result<Vec<_>, _>>()?
                    } else {
                        Vec::new()
                    };
                    match runtime.capture_pixmap_publication_prefix(namespace) {
                        Ok(prefix) => pixmap_publication_prefix = prefix,
                        Err(_) => pixmap_prefix_refused = true,
                    }
                    let released_dma_bufs = runtime.take_retired_pixmap_registrations(namespace);
                    (
                        output,
                        cpu_buffer_updates,
                        dri3_pixmap_import,
                        dri3_fence_import,
                        present_submission,
                        software_present_submission,
                        released_dma_bufs,
                        released_fence.into_iter().collect::<Vec<_>>(),
                        server_reply_fds,
                        surface_output_reservations,
                        surface_routes,
                        present_configure,
                    )
                }
                Err(error) => {
                    parse_failed = true;
                    (
                        dispatch_x11_parse_error(dispatch_context, request_minor_code, error),
                        Vec::new(),
                        None,
                        None,
                        None,
                        None,
                        Vec::new(),
                        Vec::new(),
                        Vec::new(),
                        Vec::new(),
                        Vec::new(),
                        None,
                    )
                }
            };
            // Validation is complete; opening the pinned device must not hold
            // the authority lock or substitute a newer connection generation.
            if output.outputs.iter().any(|item| matches!(item,
                crate::XClientOutput::Reply(crate::XClientReply::Dri3Open { .. })))
            {
                match state.open_render_device_fd() {
                    Ok(fd) => server_reply_fds.push(fd),
                    Err(_) => output.outputs = vec![crate::XClientOutput::Error(crate::XClientError {
                        code: crate::XErrorCode::BadImplementation,
                        sequence, resource_id: 0,
                        minor_code: u16::from(crate::X_DRI3_OPEN_MINOR_OPCODE),
                        major_code: crate::X_DRI3_MAJOR_OPCODE,
                    })],
                }
            }
            state.notify_pixmap_progress()?;
            let published = state.publish_pixmap_prefix(&pixmap_publication_prefix);
            {
                let mut runtime = state.runtime.lock()
                    .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?;
                runtime.release_pixmap_publication_prefix(pixmap_publication_prefix);
                released_dma_bufs.extend(runtime.take_retired_pixmap_registrations(namespace));
                released_dma_bufs.sort_unstable();
                released_dma_bufs.dedup();
            }
            state.notify_pixmap_progress()?;
            state.release_exported_pixmaps()?;
            if (!published? || pixmap_prefix_refused)
                && !output.outputs.iter().any(|item| matches!(item, crate::XClientOutput::Error(_)))
            {
                output.outputs.retain(|item| !matches!(item, crate::XClientOutput::Reply(_)));
                server_reply_fds.clear();
                output.outputs.push(crate::XClientOutput::Error(crate::XClientError {
                    code: crate::XErrorCode::BadAlloc,
                    sequence,
                    resource_id: 0,
                    minor_code: request_minor_code,
                    major_code: major_opcode,
                }));
            }
            let observed_received_fds = received_fds
                .iter()
                .map(OwnedFd::try_clone)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| {
                    X11SetupSocketError::new(format!(
                        "failed to retain received X11 descriptor for observation: {error}"
                    ))
                })?;
            pending_observation = Some(X11DispatchObservation {
                transaction,
                client,
                admission: admission_lease.as_ref().map(|lease| lease.context()),
                resource_id_range,
                sequence,
                major_opcode,
                minor_opcode: request_minor_code,
                request_stage,
                failure: parse_failed.then_some(X11ObservedDispatchFailure::ParseRejected),
                result: output,
                surface_routes,
                surface_output_reservations,
                cpu_buffer_updates,
                received_fd_count: received_fds.len(),
                received_fds: observed_received_fds,
                dri3_pixmap_import,
                dri3_fence_import,
                present_submission,
                software_present_submission,
                released_dma_bufs,
                released_fences,
                server_reply_fd_count: server_reply_fds.len(),
            });
            dispatch_complete = true;
            let output = &mut pending_observation
                .as_mut()
                .expect("completed dispatch observation")
                .result;
            // The requester must receive its immediate MSC answer before any
            // later reply. Peers use their own connection's event sequence.
            let mut peer_msc_deliveries = Vec::new();
            for mut delivery in pending_msc_deliveries {
                if delivery.recipient == client {
                    set_x11_protocol_event_sequence(&mut delivery.event, sequence);
                    output.outputs.push(crate::XClientOutput::Event(delivery.event));
                } else {
                    peer_msc_deliveries.push(delivery);
                }
            }
            if let Some(candidate) = pending_metadata_candidate {
                protocol_routing
                    .as_ref()
                    .expect("metadata route registry")
                    .emit_metadata_candidate(candidate)
                    .map_err(|error| {
                        X11SetupSocketError::client_failure(format!(
                            "failed to publish reduced X11 metadata: {error:?}"
                        ))
                    })?;
            }
            // Dispatch effects are already detached from the runtime. Arbitration
            // may now wait without letting another dispatch steal those effects.
            if let Some((identity, local_admitted, anchor)) = explicit_pointer_completion {
                let control = protocol_routing
                    .as_ref()
                    .and_then(|routing| routing.explicit_pointer_grabs.as_ref())
                    .ok_or_else(|| {
                        X11SetupSocketError::client_failure("explicit pointer-grab control disappeared")
                    })?;
                let admission = client_admission.ok_or_else(|| {
                    X11SetupSocketError::client_failure("explicit pointer-grab admission disappeared")
                })?;
                let activated = local_admitted
                    && matches!(
                        control.request(
                            admission,
                            crate::XAuthorityExplicitPointerGrabRequestKind::Activate { identity }
                        ),
                        Ok(crate::XAuthorityExplicitPointerGrabResponse::Activated)
                    );
                let still_owned = {
                    let runtime = lock_x11_request_runtime(&state.runtime, &state.control_runtime_pending)?;
                    let current = runtime.input_authority_mut().pointer_grab(namespace);
                    let same_surface = match (anchor, current) {
                        (crate::XAuthorityExplicitPointerGrabAnchor::AdmissionDefault, _) => true,
                        (crate::XAuthorityExplicitPointerGrabAnchor::Surface(surface), Some(grab)) => {
                            runtime
                                .window_presentation_root_and_offset(namespace, grab.window)
                                .is_ok_and(|(_, current, _, _)| current == surface)
                                && runtime
                                    .window_map_state(namespace, grab.window)
                                    .is_ok_and(|state| state == crate::XMapState::Viewable)
                        }
                        _ => false,
                    };
                    let mut input = runtime.input_authority_mut();
                    let same = input.pointer_grab(namespace).is_some_and(|grab| {
                        grab.owner == client.raw() && grab.route_lease == Some(identity)
                    });
                    let same_epoch = protocol_routing.as_ref().is_some_and(|routing| {
                        routing.input_control_epoch.load(Ordering::Acquire) == identity.control_epoch
                    });
                    if (!activated || !same_epoch || !same_surface) && same {
                        input.ungrab_pointer(namespace, client.raw());
                    }
                    same && same_epoch && same_surface
                };
                if !activated || !still_owned {
                    for output in &mut output.outputs {
                        if let crate::XClientOutput::Reply(crate::XClientReply::GrabStatus {
                            status, ..
                        }) = output
                        {
                            *status = 1;
                        }
                    }
                    let _ = control.request(
                        admission,
                        crate::XAuthorityExplicitPointerGrabRequestKind::Abort { identity },
                    );
                }
            }
            if let Some(identity) = explicit_pointer_release_completion {
                x11_finish_explicit_pointer_release(protocol_routing.as_ref(), client_admission, identity)?;
            }
            if let Some(routing) = protocol_routing.as_ref() {
                if let Some((window, geometry)) = present_configure {
                    let events = route_x11_present_configure(
                        routing,
                        client,
                        sequence,
                        window,
                        geometry,
                    )?;
                    output.outputs.splice(
                        0..0,
                        events.into_iter().map(crate::XClientOutput::Event),
                    );
                }
                route_x11_dispatch_protocol_outputs(
                    state,
                    routing,
                    namespace,
                    client,
                    output,
                )?;

                // The destroyed window is named by the notification that was
                // just routed, which keeps this independent of where the
                // request was decoded. Retiring the entries now stops a reused
                // XID from inheriting a previous window's subscribers.
                let retired = output
                    .outputs
                    .iter()
                    .filter_map(|entry| match entry {
                        crate::XClientOutput::Event(crate::XClientEvent::DestroyNotify {
                            event,
                            window,
                            ..
                        }) if event == window => Some(*window),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                for window in retired {
                    routing
                        .remove_window_parent(client, window)
                        .map_err(|error| {
                            X11SetupSocketError::new(format!(
                                "failed to remove X11 window hierarchy: {error}"
                            ))
                        })?;
                    // A destroyed window cannot receive a selection event, and
                    // its id may be reissued to the next client that asks for
                    // one; a subscription left behind would deliver to whoever
                    // inherits it.
                    routing
                        .remove_xfixes_selection_window(window)
                        .map_err(|error| {
                            X11SetupSocketError::new(format!(
                                "failed to retire XFixes selection subscriptions: {error}"
                            ))
                        })?;
                    routing.remove_core_event_window(window).map_err(|error| {
                        X11SetupSocketError::new(format!(
                            "failed to remove core X11 event subscriptions: {error}"
                        ))
                    })?;
                }
            } else {
                let selections = core_event_selections.lock().map_err(|_| {
                    X11SetupSocketError::new("X11 core event selection lock poisoned")
                })?;
                filter_local_core_lifecycle_events(&selections, output);
            }
            if std::env::var_os("SOPHIA_X11_AUTHORITY_TRACE").is_some() {
                let replies = output
                    .outputs
                    .iter()
                    .filter(|item| matches!(item, crate::XClientOutput::Reply(_)))
                    .count();
                let errors = output
                    .outputs
                    .iter()
                    .filter(|item| matches!(item, crate::XClientOutput::Error(_)))
                    .count();
                let events = output
                    .outputs
                    .iter()
                    .filter(|item| matches!(item, crate::XClientOutput::Event(_)))
                    .count();
                let (first_error_code, first_error_resource) = output
                    .outputs
                    .iter()
                    .find_map(|item| match item {
                        crate::XClientOutput::Error(error) => {
                            Some((error.code.wire_code(), error.resource_id))
                        }
                        _ => None,
                    })
                    .unwrap_or((0, 0));
                // Reported at the level an operator already runs at. The block
                // is opt-in behind an environment variable, so demanding a
                // raised global level as well means the trace can only be had by
                // changing every other target's level too -- which silences the
                // telemetry a physical gate polls for, or floods the run.
                tracing::info!(
                    // The client is what makes a sequence mean anything. A
                    // browser opens several connections at once and each numbers
                    // its own requests from one, so without this a trace reads as
                    // one client contradicting itself.
                    "sophia_x11_dispatch schema=3 client={} sequence={} major={} minor={} request_len={} parse_failed={} detail_redacted={} replies={} errors={} events={} response={} error_code={} error_resource={:#x}",
                    client.raw(),
                    sequence,
                    major_opcode,
                    request_minor_code,
                    request.len(),
                    parse_failed,
                    request_stage != X11ObservedRequestStage::Other,
                    replies,
                    errors,
                    events,
                    output.response.is_some(),
                    // Which refusal, and what it named. `errors=1` says a request
                    // was refused; without these it does not say why, and a
                    // client that retries the same refusal seven times and gives
                    // up looks identical to one that simply stopped asking.
                    first_error_code,
                    first_error_resource,
                );
            }
            let encoded_outputs = output.encoded_outputs(setup.byte_order);
            let receipt = observer(pending_observation.take().expect("one observation per allocated ticket"))?;
            if let Some(receipt) = receipt { last_published_observation = Some(receipt); }
            {
                let mut output_stream = lock_x11_non_control_output(
                    &output_stream,
                    &output_control_pending,
                )?;
                if !encoded_outputs.is_empty() || !server_reply_fds.is_empty() {
                    for (index, bytes) in encoded_outputs.into_iter().enumerate() {
                        let fds = if index == 0 {
                            core::mem::take(&mut server_reply_fds)
                        } else {
                            Vec::new()
                        };
                        let record = X11SocketOutputRecord::new(bytes, fds)?;
                        if let Err(error) =
                            write_x11_socket_output_record(&mut output_stream, record)
                        {
                            if is_x11_client_disconnect(&error) {
                                return Ok(());
                            }
                            return Err(X11SetupSocketError::new(format!(
                                "failed to write X11 output: {error}"
                            )));
                        }
                    }
                    debug_assert!(server_reply_fds.is_empty());
                    if let Err(error) = output_stream.flush() {
                        if matches!(
                            error.kind(),
                            ErrorKind::BrokenPipe
                                | ErrorKind::ConnectionReset
                                | ErrorKind::UnexpectedEof
                        ) {
                            return Ok(());
                        }
                        return Err(X11SetupSocketError::new(format!(
                            "failed to flush X11 output: {error}"
                        )));
                    }
                }
                // Publish the request sequence while holding the same lock
                // used by every asynchronous event writer. Otherwise a
                // writer can snapshot the old value, wait behind this reply,
                // and emit a backwards sequence after it.
                event_sequence.store(sequence, Ordering::Release);
            }
            for delivery in peer_msc_deliveries {
                protocol_routing
                    .as_ref()
                    .expect("MSC subscription has a route registry")
                    .route_protocol(delivery.recipient, delivery.event)
                    .map_err(|error| X11SetupSocketError::new(format!(
                        "failed to deliver peer Present NotifyMSC: {error}"
                    )))?;
            }
        }
        Ok(())
    })();

    // Resolve the allocated ticket before cleanup allocates another. Completed
    // effects survive a later client failure, including mutations of peer-owned
    // windows in a shared namespace. Partial dispatch is fatal, never an empty
    // success that would certify missing authority effects.
    let pending_publication_result = if let Some(observation) = failed_x11_dispatch_observation(
        pending_observation.take(),
        dispatch_started,
        dispatch_complete,
    ) {
        let published = observer(observation).map(|_| ());
        if dispatch_started && !dispatch_complete {
            published.and(Err(X11SetupSocketError::new(
                "X11 dispatch ended before its effects were published",
            )))
        } else {
            published
        }
    } else {
        Ok(())
    };
    let writer_result: Result<(), X11SetupSocketError> = (|| {
        if let Some(writer) = input_writer {
            writer.stop.store(true, Ordering::Release);
            writer.thread.join().map_err(|_| {
                X11SetupSocketError::new("X11 input event writer thread panicked")
            })??;
        }
        if let Some(writer) = control_writer {
            writer.stop.store(true, Ordering::Release);
            writer
                .thread
                .join()
                .map_err(|_| X11SetupSocketError::new("X11 control writer thread panicked"))??;
        }
        if let Some(writer) = protocol_writer {
            writer.stop.store(true, Ordering::Release);
            writer.thread.join().map_err(|_| {
                X11SetupSocketError::new("X11 protocol event writer thread panicked")
            })??;
        }
        Ok(())
    })();
    state
        .runtime
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
        .input_authority_mut()
        .cleanup_owner(client.raw());
    if let Some(routing) = protocol_routing.as_ref() {
        let mut pointers = routing.pointer_state.lock()
            .map_err(|_| X11SetupSocketError::new("X11 pointer state lock poisoned"))?;
        let authority = routing.input_authority.lock()
            .map_err(|_| X11SetupSocketError::new("X11 input authority lock poisoned"))?;
        if !authority.query_namespace_active(namespace) {
            pointers.retain(|(owner, _), _| *owner != namespace);
        }
    }
    state.runtime.lock()
        .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
        .release_client_device_bundle(client.raw());
    let client_lease = state.release_client(client)?;
    debug_assert_eq!(client_lease.resource_id_range, resource_id_range);
    let mut release = release_x11_client_lease(state, namespace, client_lease)?;
    release.released_dma_bufs.extend(state.runtime.lock()
        .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
        .take_retired_pixmap_registrations(namespace));
    release.released_dma_bufs.sort_unstable();
    release.released_dma_bufs.dedup();
    // The selections this client owned ended with it, and its watchers are
    // owed that. Drained before the subscriptions are retired below, because
    // those are what name the recipients.
    let retired_selections = core::mem::take(&mut release.retired_selection_ownerships);
    if let Some(routing) = protocol_routing.as_ref() {
        for retired in retired_selections {
            // Routed under the namespace the ownership belonged to, not this
            // connection's: the queue is shared across namespaces.
            let Some(owner_namespace) = retired.current.namespace else {
                continue;
            };
            let subtype = selection_change_subtype(retired.kind);
            for (recipient, window) in routing
                .xfixes_selection_subscribers(owner_namespace, retired.current.selection, subtype)
                .map_err(|error| {
                    X11SetupSocketError::new(format!(
                        "failed to inspect XFixes selection subscriptions: {error}"
                    ))
                })?
            {
                // The departed client is not told its own departure.
                if recipient == client {
                    continue;
                }
                if let Err(error) = routing.route_protocol(
                    recipient,
                    crate::XClientEvent::XfixesSelectionNotify {
                        sequence: 0,
                        subtype,
                        window,
                        owner: crate::XResourceId::NONE,
                        selection: retired.current.selection,
                        time: x11_server_time_msec(),
                        selection_time: retired.current.selection_timestamp,
                    },
                ) {
                    if let XServerFrontendRouteError::ClientQueueFull { client: stalled } = error {
                        routing
                            .disconnect_saturated_recipient(stalled)
                            .map_err(|error| {
                                X11SetupSocketError::new(format!(
                                    "failed to end a stalled XFixes watcher: {error}"
                                ))
                            })?;
                    } else if !x11_recipient_is_gone(&error) {
                        return Err(X11SetupSocketError::new(format!(
                            "failed to route a departed peer's selection change: {error}"
                        )));
                    }
                }
            }
        }
        // The departing client's own subscriptions go with it, so a reissued
        // client id cannot inherit what this one was watching.
        routing
            .remove_xfixes_selection_client(client)
            .map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to retire a departed peer's XFixes subscriptions: {error}"
                ))
            })?;
    }
    state.notify_pixmap_progress()?;
    state.release_exported_pixmaps()?;
    if let Some(routing) = protocol_routing.as_ref() {
        const STRUCTURE_NOTIFY_MASK: u32 = 1 << 17;
        const SUBSTRUCTURE_NOTIFY_MASK: u32 = 1 << 19;
        for window in &release.destroyed_windows {
            // A window vanishing because its client went away is still a
            // window vanishing, and the clients watching it are still owed the
            // notification. Losing a peer is the ordinary way a window manager
            // learns a top-level is gone.
            //
            // Notify before retiring the subscriptions: they are what names the
            // recipients, so clearing them first would deliver to nobody. The
            // departed client needs nothing, and route_protocol tolerates a
            // recipient that has also gone.
            let parent = routing.window_parent(*window).map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to resolve a disconnected X11 window's parent: {error}"
                ))
            })?;
            for (target, mask) in [
                (Some(*window), STRUCTURE_NOTIFY_MASK),
                (parent, SUBSTRUCTURE_NOTIFY_MASK),
            ] {
                let Some(target) = target else {
                    continue;
                };
                let subscribers =
                    routing
                        .core_event_subscribers(target, mask)
                        .map_err(|error| {
                            X11SetupSocketError::new(format!(
                                "failed to inspect disconnected X11 subscriptions: {error}"
                            ))
                        })?;
                for recipient in subscribers {
                    routing
                        .route_protocol(
                            recipient,
                            crate::XClientEvent::DestroyNotify {
                                sequence: 0,
                                event: target,
                                window: *window,
                            },
                        )
                        .map_err(|error| {
                            X11SetupSocketError::new(format!(
                                "failed to route a disconnected X11 destroy: {error}"
                            ))
                        })?;
                }
            }
        }
        // Retire the subscriptions only once every notification is routed. A
        // destroyed window is frequently the parent that another destroyed
        // window addresses its SubstructureNotify to, so removing them as the
        // loop went delivered those to nobody. Destruction order alone would
        // mask this -- children precede their parents -- but that is too
        // fragile to rest on, and a separate pass cannot be got wrong.
        for window in &release.destroyed_windows {
            routing
                .remove_xfixes_selection_window(*window)
                .map_err(|error| {
                    X11SetupSocketError::new(format!(
                        "failed to retire a disconnected peer's XFixes selection subscriptions: {error}"
                    ))
                })?;
            routing
                .remove_core_event_window(*window)
                .map_err(|error| {
                    X11SetupSocketError::new(format!(
                        "failed to remove disconnected X11 event subscriptions: {error}"
                    ))
                })?;
        }
    }
    drop(route_registration);
    let cleanup_observer_result = if release.removed_surfaces.is_empty()
        && release.released_dma_bufs.is_empty()
        && release.released_fences.is_empty()
    {
        Ok(())
    } else {
        sequence = sequence.wrapping_add(1);
        let transaction = state.allocate_transaction()?;
        let mut response = XAuthorityResponsePacket::accepted(transaction);
        response.removed_surfaces = release.removed_surfaces;
        let cleanup = XDispatchResult {
            response: Some(response),
            outputs: Vec::new(),
            metadata_candidates: Vec::new(),
        };
        
        observer(X11DispatchObservation {
            transaction,
            client,
            admission: admission_lease.as_ref().map(|lease| lease.context()),
            resource_id_range,
            sequence,
            major_opcode: 0,
            minor_opcode: 0,
            request_stage: X11ObservedRequestStage::DisconnectCleanup,
            failure: None,
            result: cleanup,
            surface_routes: Vec::new(),
            surface_output_reservations: Vec::new(),
            cpu_buffer_updates: Vec::new(),
            received_fd_count: 0,
            received_fds: Vec::new(),
            dri3_pixmap_import: None,
            dri3_fence_import: None,
            present_submission: None,
            software_present_submission: None,
            released_dma_bufs: release.released_dma_bufs,
            released_fences: release.released_fences,
            server_reply_fd_count: 0,
        }).map(|_| ())
    };
    let admission_result = admission_lease.as_mut().map_or(Ok(()), |lease| {
        lease.revoke().map_err(|error| {
            X11SetupSocketError::new(format!("failed to revoke X11 client admission: {error}"))
        })
    });
    pending_publication_result?;
    result?;
    writer_result?;
    cleanup_observer_result?;
    admission_result
}

#[cfg(all(test, unix))]
mod surface_generation_tests {
    use super::*;

    #[test]
    fn rejected_candidate_is_reusable_but_admitted_xid_recreation_advances() {
        let mut ledger = X11SurfaceGenerationLedger::default();

        let first = ledger.candidate(0x220001).unwrap();
        assert_eq!(first, SurfaceId::new(0x220001, 1));
        assert_eq!(ledger.candidate(0x220001).unwrap(), first);

        ledger.admit(first).unwrap();
        let replacement = ledger.candidate(0x220001).unwrap();
        assert_eq!(replacement, SurfaceId::new(0x220001, 2));
        assert_ne!(replacement, first);
        ledger.admit(replacement).unwrap();
        assert_eq!(
            ledger.candidate(0x220001).unwrap(),
            SurfaceId::new(0x220001, 3)
        );
    }

    #[test]
    fn ledger_rejects_stale_or_skipped_admission() {
        let mut ledger = X11SurfaceGenerationLedger::default();

        assert!(ledger.admit(SurfaceId::new(7, 2)).is_err());
        ledger.admit(SurfaceId::new(7, 1)).unwrap();
        assert!(ledger.admit(SurfaceId::new(7, 1)).is_err());
        assert!(ledger.admit(SurfaceId::new(7, 3)).is_err());
    }

    #[test]
    fn generation_exhaustion_fails_closed() {
        let mut ledger = X11SurfaceGenerationLedger::default();
        ledger.admitted.insert(9, u32::MAX);

        assert!(ledger.candidate(9).is_err());
    }
}
