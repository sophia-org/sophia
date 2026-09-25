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
    /// Who may fake input, decided by the same instance that decided who may
    /// connect, and separately from it.
    injection_policy: Option<Arc<dyn crate::XServerFrontendInjectionPolicy>>,
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

/// What a dispatch that started and did not complete means for the service.
///
/// The observation has already been published as failed by the time this is
/// asked, so nothing here certifies a partial dispatch as success; what is
/// decided is whose fault it was. The dispatch closure returns `Ok(())` only
/// on a departure -- the peer went away while its request was in flight, the
/// XTEST barrier being the ordinary place -- and that is the client's
/// disconnect, which the worker reaper contains. Any other way of ending
/// mid-dispatch is the service's own failure and stays fatal. Until this was
/// separated a departed client and a broken service raised the same
/// unclassified error, and one `xdotool` click ended the frontend.
#[cfg(unix)]
pub(crate) fn partial_dispatch_error(departed: bool) -> X11SetupSocketError {
    if departed {
        X11SetupSocketError::client_disconnect(
            "X11 client departed before its dispatch published its effects",
        )
    } else {
        X11SetupSocketError::new("X11 dispatch ended before its effects were published")
    }
}

/// A failed post-dispatch delivery still owes the complete authority effects.
/// Only a request that never dispatched may retire an empty ordering ticket.
#[cfg(unix)]
fn failed_x11_dispatch_observation(
    pending: Option<X11DispatchObservation>,
    started: bool,
    complete: bool,
    departed: bool,
) -> Option<X11DispatchObservation> {
    pending.map(|mut observation| {
        if !complete {
            observation.failure = Some(if !started {
                X11ObservedDispatchFailure::DispatchAborted
            } else if departed {
                X11ObservedDispatchFailure::ClientDeparted
            } else {
                X11ObservedDispatchFailure::UnpublishedEffects
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

/// This client's registration as a query owner for its namespace.
///
/// Owned rather than ordered. A standalone client has no route registration
/// whose drop would clean this up, and the device pin releases only its device
/// bundle, so an early return after registering left the namespace reporting an
/// owner that never finished starting. Ordinary teardown takes it back here.
/// Private teardown requests closure on its exact retained lifecycle owner;
/// that owner performs cleanup under common and preserves any interruption.
#[cfg(unix)]
struct X11QueryOwner<'a> {
    private: Option<(PrivateLifecycleOwner, PrivateLifecycleGate)>,
    finished: bool,
    runtime: &'a Mutex<XAuthorityRuntime>,
    client: XServerFrontendClientId,
}

#[cfg(unix)]
impl<'a> X11QueryOwner<'a> {
    fn register(
        runtime: &'a Mutex<XAuthorityRuntime>,
        namespace: NamespaceId,
        client: XServerFrontendClientId,
        private: Option<(PrivateLifecycleOwner, PrivateLifecycleGate)>,
    ) -> Result<Self, X11SetupSocketError> {
        if let Some((owner, gate)) = &private {
            owner.register_query_gate(gate, namespace, client).map_err(|error| X11SetupSocketError::new(format!("private query owner refused: {error:?}")))?;
            return Ok(Self { runtime, client, private, finished: false });
        }
        runtime
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
            .input_authority_mut()
            .register_query_client(namespace, client.raw());
        Ok(Self { runtime, client, private, finished: false })
    }
}

/// Everything one client connection owns that has to be given up in order.
///
/// An aggregate rather than two locals, because the order is the guarantee and
/// two `let`s only have it by accident: fields are given up in declaration
/// order, so the workers stop and join before anything they were serving is
/// taken back. Locals are given up in reverse, which had the query
/// registration going first while the writers it served were still running.
#[cfg(unix)]
struct X11ClientLifetime<'a> {
    /// Given up first.
    writers: X11ClientWriters,
    /// Kept after every worker stops, including partial startup. Its socket
    /// is independent of output serialization and belongs to this runner's
    /// supervisor even when setup completed after that runner was prepared.
    #[allow(dead_code)]
    watchdog_transport: Option<private_watchdog::PrivateWatchdogTransport>,
    /// Given up after them.
    ///
    /// Never read: it is held for what losing it does, which is take this
    /// client's query registration back once nothing is still serving it.
    #[allow(dead_code)]
    query_owner: X11QueryOwner<'a>,
}

#[cfg(unix)]
impl X11QueryOwner<'_> {
    fn finish(&mut self) -> Result<(), X11SetupSocketError> {
        if self.finished { return Ok(()) }
        if let Some((owner, gate)) = &self.private {
            gate.close();
            owner.drive_every_slot().map_err(|error| X11SetupSocketError::new(format!("private cleanup unavailable: {error:?}")))?;
        } else {
            self.runtime.lock().map_err(|_| X11SetupSocketError::new("X11 runtime unavailable"))?.input_authority_mut().cleanup_owner(self.client.raw());
        }
        self.finished = true;
        Ok(())
    }
}
impl Drop for X11QueryOwner<'_> {
    fn drop(&mut self) {
        if let Some((_, gate)) = &self.private { gate.close(); } else { let _ = self.finish(); }
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
    // Register before any setup read/write and before a worker can escape.
    // This local covers setup failure; the aggregate below then keeps it
    // until all workers have stopped. No output/common/X guard is held here.
    let watchdog_transport = client_routing
        .as_ref()
        .and_then(|routing| routing.input_recovery.watchdog.get())
        .map(|registrar| {
            let socket = stream.try_clone().map_err(|error| {
                X11SetupSocketError::new(format!("failed to clone watchdog socket: {error}"))
            })?;
            registrar
                .attach_transport(socket)
                .map_err(|(cause, _socket)| {
                    X11SetupSocketError::client_failure(format!(
                        "private connection supervisor refused setup: {cause:?}"
                    ))
                })
        })
        .transpose()?;
    let X11ClientAdmissionContext {
        authorization,
        admission_policy,
        injection_policy,
        worker_admission,
    } = admission;
    if admission_policy.is_none() && client_routing.as_ref().is_some_and(|routing| routing.input_recovery.lifecycle.get().is_some()) {
        return Err(X11SetupSocketError::new("private instance requires a current admission policy"));
    }
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
    // Issued once, at setup, from the same admission that admitted this
    // connection, so discovery and every request answer from one decision.
    //
    // Filled in below rather than here. The issuer answers from the live
    // admitted row for this connection, and that row does not exist until the
    // connection has been registered and its private lifecycle attached, so
    // asking at this point is asking about a client the authority has not met
    // and is refused every time.
    let mut injector: Option<Box<dyn crate::XTestInjector>> = None;
    let client_lease = setup_lease.ok_or_else(|| {
        X11SetupSocketError::new("Sophia X Server Frontend did not retain a setup client lease")
    })?;
    let client = client_lease.client;
    // Setup allocation, before registry attachment and worker exposure. A
    // repeated connection preserves the focus already applied in its namespace.
    state.runtime.lock().map_err(|_| {
        X11SetupSocketError::new("X11 authority runtime unavailable during focus preparation")
    })?.prepare_input_focus_namespace(namespace);
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
    let output_stream = X11ClientOutput::shared(
        stream.try_clone().map_err(|error| {
            X11SetupSocketError::new(format!("failed to clone X11 output socket: {error}"))
        })?,
        client.raw(),
    );
    let output_control_pending = Arc::new(AtomicUsize::new(0));
    // One per connection, beside the output it governs. Every post-exposure
    // writer of this socket is given it, so a wire left holding the beginning
    // of an event nobody can finish stops all of them and not just whoever
    // discovered it.
    let output_wire = Arc::new(X11WirePermission::open());
    let protocol_routing = client_routing.clone();
    let (route_registration, input_receiver, control_channels, protocol_receiver, input_watermark) =
        if let Some(routing) = client_routing {
            if let Err(error) = routing.bind_runtime(&state.runtime) {
                let _ = state.release_client(client);
                return Err(error);
            }
            let admission = admission_lease.as_ref().map(|lease| lease.context());
            let (registration, channels) = match routing
                .register_client_in_namespace(client, admission, Some(namespace))
            {
                Ok(registration) => registration,
                Err(error) => {
                    let _ = state.release_client(client);
                    let message = format!("failed to register X11 client route: {error}");
                    // A FULL STORE IS THIS CONNECTION'S ANSWER, NOT THE
                    // SERVICE'S ENDING. Registration can refuse for two
                    // unlike reasons. A poisoned registry is the instance
                    // saying it can no longer be trusted to route anybody,
                    // and ending is the honest response to that. Having no
                    // place left is the instance saying it is full, which is
                    // a fact about this one admission and about nothing else:
                    // every connection already served is still being served
                    // correctly, and the next admission may well find room.
                    //
                    // Unclassified, these were the same, so four ordinary
                    // departures on an instance admitting four left the fifth
                    // connection ending the whole private service -- the
                    // class t130 named, a per-client condition the service
                    // does not survive. Named as a client failure, the
                    // frontend disconnects this one and keeps serving.
                    //
                    // THE EVIDENCE CUSTODY IS THE SAME KIND OF FULL, and it
                    // is the next one an instance meets: once departed
                    // connections give their places back during the run, the
                    // custody they also hold is what the fifth admission
                    // runs out of instead. Its reclamation is not reachable
                    // during the run at all -- a custody is retired by the
                    // invocation completion, which the maintenance keeper
                    // drives after the invocation has ended -- so until that
                    // is live this refusal is the honest answer, and it is
                    // still an answer about one connection.
                    return Err(match error {
                        XServerFrontendRouteError::ContinuationUnavailable { .. }
                        | XServerFrontendRouteError::EvidenceCustodyUnavailable { .. } => {
                            X11SetupSocketError::client_failure(message)
                        }
                        _ => X11SetupSocketError::new(message),
                    });
                }
            };
            // BOUND HERE, BEFORE THE FIRST THING THAT CAN REFUSE.
            //
            // This is the one place where the accepted stream and the
            // registration minted for it are both in hand and neither has been
            // anywhere else, which is what makes the pairing sound rather than
            // asserted. It is also the first instruction after publication:
            // the row is live from the line above, so a capsule can already be
            // on this queue, and every attachment below can refuse. A binding
            // placed after them left each of those refusals dropping the
            // receiver -- the place reserved for this connection survived, and
            // the accepted work it was reserved for did not. An empty place is
            // not custody of anything.
            //
            // What to do with a refused binding is decided in the registry,
            // beside the other rules about accepted work. NOTHING IS STARTED
            // HERE: binding is preparation, and no ordered worker exists.
            // THE AUTHORITATIVE STOP, minted here and given to the binding,
            // so the serving owner a later promotion makes carries this one
            // and a registered worker started on it answers to this one.
            let ordered_stop = Arc::new(AtomicBool::new(false));
            if registration
                .bind_ordered_output_stoppable(
                    channels.ordered,
                    &output_stream,
                    &output_wire,
                    &output_control_pending,
                    &ordered_stop,
                )
                .is_err()
            {
                // This registration is new, so it holds no custody and this
                // cannot happen. It refuses rather than replacing a custody
                // that may already hold accepted capsules.
                let _ = state.release_client(client);
                return Err(X11SetupSocketError::new(
                    "X11 ordered output was already bound for this client".to_string(),
                ));
            }
            // Keep the actual connection projection with this exact route
            // registration before any writer can observe or mutate it. Private
            // preparation may come before or after this setup edge.
            if let Some(context) = admission { routing.attach_private_lifecycle(&registration, context)?; }
            // The earliest point the issuer can answer about this connection:
            // it is registered, its lifecycle is attached, and the admitted
            // row the issuance validates against now exists.
            //
            // A refusal is not a connection error and must not be treated as
            // one. Most clients may connect and may not inject, and such a
            // client goes on being served everything else; what it loses is
            // XTEST, which it is then told is absent rather than left to
            // discover one refusal at a time.
            injector = admission_lease
                .as_ref()
                .zip(injection_policy.as_ref())
                .and_then(|(lease, policy)| {
                    policy
                        .issue(lease.context(), crate::X_TEST_INJECTION_DEVICE)
                        .ok()
                });
            routing.attach_connection_state(
                &registration,
                namespace,
                core_event_selections.clone(),
                focused_surface_window.clone(),
            ).map_err(|error| X11SetupSocketError::new(format!(
                "failed to register X11 applied connection state: {error:?}"
            )))?;
            routing.input_recovery.attach(client, stream.try_clone().map_err(|error|
                X11SetupSocketError::new(format!("failed to clone recovery socket: {error}")))?)
                .map_err(|error| X11SetupSocketError::new(error.to_string()))?;
            // READINESS FOR A REGISTERED ORDERED-OUTPUT WORKER, published once
            // on this connection's own record, only now that its setup is
            // complete: the byte order the handshake fixed, the sequence this
            // dispatch advances, the stop the binding carries, and a second
            // independent handle on the socket for whoever must interrupt a
            // blocked write without the output mutex. NOTHING IS STARTED
            // HERE. A handle that cannot be taken publishes nothing, so no
            // worker will ever start for this connection and nothing exists
            // that could not be collected; the accepted work stays retained.
            if let Ok(interrupt) = stream.try_clone() {
                let _ = registration.publish_worker_readiness(PrivateWorkerReadiness {
                    byte_order: setup.byte_order,
                    sequence: event_sequence.clone(),
                    stop: ordered_stop,
                    interrupt,
                });
            }
            let input_watermark = channels.input_watermark.clone();
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
                Some(input_watermark),
            )
        } else {
            (None, input_receiver, control_channels, None, None)
        };
    let control_cleanup_source = match (protocol_routing.as_ref(), route_registration.as_ref()) {
        (Some(routing), Some(registration)) => routing.prepare_control_source(registration, state, resource_id_range, PrivateControlClientTables {
            windows: surface_windows.clone(), rules: metadata_rules.clone(), generations: metadata_generations.clone(),
        })?,
        _ => None,
    };
    let mut last_published_observation = None;
    let standalone_query_authority = if protocol_routing.is_none() {
        Some(state.runtime.lock()
            .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
            .shared_input_authority())
    } else { None };
    // Declared before the first spawn, so every path out from here owns the
    // shutdown of whatever has already started, and its own handle on the
    // socket comes with it -- a writer blocked in a write holds the mutex that
    // anything else would have to take first. Taken before any worker exists,
    // so a descriptor that cannot be had refuses the connection rather than
    // starting workers whose shutdown has no way to reach them.
    //
    // And taken before this client is registered as a query owner, so that
    // refusing leaves nothing registered to roll back. A standalone client has
    // no route registration whose drop would clean that up, and the device pin
    // releases only its device bundle, so a refusal after it would leave the
    // namespace reporting an owner that never finished starting.
    //
    // Declared after the route registration on purpose: locals drop in reverse,
    // so the writers are stopped and joined before the registration they were
    // serving goes.
    let writer_transport = X11ClientWriters::take_transport(&output_stream)?;
    // Started with the transport handle, before anything is registered, so a
    // refusal after this owns its stop through the cohort's drop.
    let output_drain = spawn_x11_output_drain(output_stream.clone(), client.raw())?;
    let private_query = if let (Some(routing), Some(registration)) = (protocol_routing.as_ref(), route_registration.as_ref()) {
        let lease = registration.lifecycle.lock().map_err(|_| X11SetupSocketError::new("private lease unavailable"))?;
        match lease.as_ref() {
            Some(lease) => {
                let owner = routing.input_recovery.lifecycle.get().ok_or_else(|| X11SetupSocketError::new("private lease lost its owner"))?;
                if !lease.belongs_to(owner) { return Err(X11SetupSocketError::new("private lease belongs to another origin")); }
                Some((owner.clone(), lease.gate()))
            }
            None => None,
        }
    } else { None };
    // Taken up only now, because its waits need the gate the query owner just
    // produced. A notifier that cannot be made leaves the client without the
    // extension rather than with a request that fails mid-way: it is told
    // XTEST is absent, which is true of it.
    let mut xtest: Option<XTestConnection> = injector
        .take()
        .and_then(|injector| {
            XTestConnection::new(
                injector,
                private_query.as_ref().map(|(_, gate)| gate.clone()),
                input_watermark.clone(),
            )
            .ok()
        });
    let mut owned = X11ClientLifetime {
        // Registered only once the handle that can end a stalled write is in
        // hand, so a refusal registers nothing that would need taking back.
        query_owner: X11QueryOwner::register(&state.runtime, namespace, client, private_query)?,
        writers: X11ClientWriters::owning(writer_transport, output_drain),
        watchdog_transport,
    };
    let writers = &mut owned.writers;
    writers.input = input_receiver
        .map(|receiver| {
            spawn_x11_input_event_writer(
                X11InputWriterState {
                    input_watermark: input_watermark.clone(),
                    stream: output_stream.clone(),
                    output_control_pending: output_control_pending.clone(),
                    output_wire: output_wire.clone(),
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
    #[cfg(all(test, unix))]
    routing_tests::m3_acceptance::writers_started(writers, protocol_routing.as_ref());
    writers.control = control_channels
        .map(|channels| {
            spawn_x11_control_writer(
                output_stream.clone(),
                output_control_pending.clone(),
                output_wire.clone(),
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
    #[cfg(all(test, unix))]
    routing_tests::m3_acceptance::writers_started(writers, protocol_routing.as_ref());
    writers.protocol = protocol_receiver
        .map(|receiver| {
            spawn_x11_protocol_event_writer(
                output_stream.clone(),
                output_control_pending.clone(),
                output_wire.clone(),
                setup.byte_order,
                event_sequence.clone(),
                client,
                receiver,
            )
        })
        .transpose()?;
    #[cfg(all(test, unix))]
    routing_tests::m3_acceptance::writers_started(writers, protocol_routing.as_ref());
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
    let mut pending_focus_publication = None::<X11PendingFocusPublication>;
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
        // BIG-REQUESTS is enabled per connection, by the request itself:
        // from the one that asked, a zero length field means a 32-bit
        // length follows. Set when the request is read rather than when
        // its reply leaves, which is the earlier of the two and the one
        // the reader needs; a client cannot use the encoding before the
        // reply anyway.
        let mut big_requests_enabled = false;
        while let Some(received) =
            read_x11_core_request(stream, setup.byte_order, big_requests_enabled)?
        {
            let major_opcode = received.major_opcode;
            let request = received.bytes;
            let framing = received.framing;
            if major_opcode == crate::X_BIG_REQUESTS_MAJOR_OPCODE
                && request.get(1) == Some(&crate::X_BIG_REQUESTS_ENABLE_MINOR_OPCODE)
            {
                big_requests_enabled = true;
            }
            let request_minor_code = if major_opcode >= 128 {
                u16::from(request[1])
            } else {
                0
            };
            let ancillary_fds = received.fds;
            let mut received_fds = Vec::new();
            loop {
                // A client that asked to be impervious is not paused by
                // another client's server grab. That is the whole of XTEST's
                // GrabControl, and the reason it exists: a harness has to be
                // able to drive a server that the client under test has
                // grabbed, and a harness that paused with everyone else could
                // never release it. Asked before the owner is read, because
                // an impervious client has no interest in who holds the grab.
                //
                // EXCEPT FOR A GRABSERVER OF ITS OWN (opcode 36), which is
                // the one request the exemption must not carry past this
                // pause. GrabServer defines no error, so a client that asked
                // for it cannot be told it failed; what the reference does
                // instead is defer, and this pause IS that deferral -- it
                // parks the client, re-reads the owner on every wake, and
                // lets the request through once the grab is free. Past it
                // there is nothing that waits, only `let _ = grab_server(..)`
                // discarding AlreadyGrabbed, so an exempted GrabServer was
                // silently dropped where every client's used to wait.
                //
                // A harness is entitled to have its OTHER requests served
                // through somebody else's grab. It is not entitled to have a
                // grab it asked for quietly thrown away.
                if xtest
                    .as_ref()
                    .is_some_and(|connection| connection.impervious)
                    && major_opcode != 36
                {
                    break;
                }
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
            // A FakeInput's delay is taken here, from the bytes, before this
            // request is numbered and before it is given a transaction, and
            // therefore before it owes an observation. That placement is the
            // whole of it. The reference sleeps before it validates detail
            // and root, so a malformed request carrying a delay waits and
            // only then answers its error; and a ticket is an ordering
            // obligation, so a request that holds one while it sleeps stops
            // every later request on every other connection from publishing
            // what it did. A second's delay must cost the client that asked
            // for it a second and cost its neighbours nothing.
            if let Some(connection) = xtest.as_mut()
                && major_opcode == crate::X_TEST_MAJOR_OPCODE
                && request.len() >= 12
                && request[1] == crate::X_TEST_FAKE_INPUT_MINOR_OPCODE
            {
                let delay = setup.byte_order.u32(&request[8..12]);
                if delay != 0 {
                    match connection.delay(stream, delay)? {
                        XTestWaitEnd::Departed => return Ok(()),
                        // Revoked while waiting: the request proceeds to be
                        // refused by an authority that no longer admits it,
                        // which emits nothing, exactly as the reference
                        // cancels a sleeping client's work.
                        XTestWaitEnd::Cancelled | XTestWaitEnd::Settled => {}
                    }
                }
            }
            sequence = sequence.wrapping_add(1);
            {
                // Dispatch can wake a peer that immediately sends an event
                // back to this client. Publish before releasing any effect,
                // not after the peer has already observed the request. The
                // protocol writer stamps and writes under this same lock,
                // so publication cannot overtake one of its older events.
                // Publication emits no bytes and must not wait for control
                // priority: this request may supersede that queued control.
                // The eventual reply still takes the ordinary output turn.
                let mut output = enter_x11_wire(&output_stream, &output_wire)?;
                // A request read is the client's activity, which the silence
                // allowance measures the absence of.
                output.note_activity();
                event_sequence.store(sequence, Ordering::Release);
            }
            let transaction = state.allocate_transaction()?;
            let dispatch_context = XDispatchContext {
                byte_order: setup.byte_order,
                namespace,
                transaction,
                // Stamped once per request so every event it generates
                // agrees, and never zero, which the wire reserves for
                // CurrentTime.
                server_time: x11_server_time_msec(),
                sequence,
                major_opcode,
                client_id: client.raw(),
                injection: if xtest.is_some() {
                    crate::XTestAdmission::Admitted
                } else {
                    crate::XTestAdmission::Absent
                },
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
            // Kept from the decode so an accepted request can be acted on
            // after the guard that validated it is released. Only the
            // dispatcher's refusal is decided under that guard.
            let mut fake_input: Option<XTestFakeInputRequest> = None;
            let mut grab_control: Option<u8> = None;
            // A WarpPointer this layer turns into motion once accepted: the
            // dispatcher moves only the position QueryPointer reads, and the
            // routed pointer follows through the requester's own injector.
            let mut warp_pointer = false;
            let mut pointer_before_warp: Option<(i16, i16)> = None;
            // A lifetime request acts on the leases this layer owns, once
            // the dispatcher has validated it (t166).
            let mut lifetime_request: Option<crate::XWireRequest> = None;
            let (
                mut output,
                cpu_buffer_updates,
                dri3_pixmap_import,
                dri3_fence_import,
                present_submission,
                software_present_submission,
                mut released_dma_bufs,
                mut released_fences,
                mut server_reply_fds,
                surface_output_reservations,
                surface_routes,
                present_configure,
            ) = match match framing {
                // Read to its end and dropped by the reader: nothing here
                // decodes it, and what it is owed is decided by its frame.
                X11RequestFraming::Refused { units } => {
                    Err(crate::XWireParseError::BeyondMaximumLength {
                        opcode: major_opcode,
                        units,
                    })
                }
                X11RequestFraming::Ordinary | X11RequestFraming::Extended => {
                    decode_x11_core_request(
                        XWireClientContext {
                            byte_order: setup.byte_order,
                            namespace,
                            transaction,
                            resource_id_range: Some(resource_id_range),
                        },
                        &request,
                    )
                }
            } {
                Ok(mut request) => {
                    fake_input = XTestFakeInputRequest::from_request(&request);
                    warp_pointer = matches!(&request, crate::XWireRequest::WarpPointer { .. });
                    if let crate::XWireRequest::XTestGrabControl { impervious } = &request {
                        grab_control = Some(*impervious);
                    }
                    if matches!(
                        &request,
                        crate::XWireRequest::ChangeSaveSet { .. }
                            | crate::XWireRequest::SetCloseDownMode { .. }
                            | crate::XWireRequest::KillClient { .. }
                    ) {
                        lifetime_request = Some(request.clone());
                    }
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
                        crate::XWireRequest::SetInputFocus { focus, revert_to, time } => Some((*focus, *revert_to, *time)),
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
                    let circulated = match &request {
                        crate::XWireRequest::CirculateWindow { window, direction } => Some((*window, *direction)),
                        _ => None,
                    };
                    let configured = match &request {
                        crate::XWireRequest::ConfigureWindow { .. } => Some(request.clone()),
                        _ => None,
                    };
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
                    // Where the pointer was, so a warp that lands where it
                    // already is (or one the protocol makes a no-op) moves
                    // nothing and reports nothing.
                    if warp_pointer {
                        pointer_before_warp = runtime.pointer_query_position(namespace);
                    }
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
                    // A client selecting SubstructureRedirect on the parent has
                    // asked to decide what happens to its children, so a map of
                    // one becomes a MapRequest to that client and the window
                    // stays unmapped. Override-redirect is the window saying it
                    // is not for a manager to place, and is never redirected.
                    const SUBSTRUCTURE_REDIRECT_MASK: u32 = 1 << 20;
                    let redirected_map = match (mapped_window, protocol_routing.as_ref()) {
                        (Some(window), Some(routing))
                            if !runtime
                                .window_override_redirect(namespace, window)
                                .unwrap_or(false) =>
                        {
                            let parent = routing.window_parent(window).map_err(|error| {
                                X11SetupSocketError::new(format!(
                                    "failed to resolve X11 map redirect parent: {error}"
                                ))
                            })?;
                            match parent {
                                Some(parent) => routing
                                    .core_event_subscribers(parent, SUBSTRUCTURE_REDIRECT_MASK)
                                    .map_err(|error| {
                                        X11SetupSocketError::new(format!(
                                            "failed to inspect X11 map redirect subscriptions: {error}"
                                        ))
                                    })?
                                    .iter()
                                    .any(|recipient| *recipient != client)
                                    .then_some((parent, window)),
                                None => None,
                            }
                        }
                        _ => None,
                    };
                    // A ConfigureWindow on a child another client manages
                    // (SubstructureRedirect selected on the parent) is that
                    // client's to decide: it hears the request as a
                    // ConfigureRequest and nothing is applied. Override-redirect
                    // is never redirected, as for a map (t198).
                    let redirected_configure = match (&configured, protocol_routing.as_ref()) {
                        (Some(crate::XWireRequest::ConfigureWindow { window, .. }), Some(routing))
                            if !runtime.window_override_redirect(namespace, *window).unwrap_or(false) =>
                        {
                            let parent = routing.window_parent(*window).map_err(|error| {
                                X11SetupSocketError::new(format!(
                                    "failed to resolve X11 configure redirect parent: {error}"
                                ))
                            })?;
                            match parent {
                                Some(parent) => routing
                                    .core_event_subscribers(parent, SUBSTRUCTURE_REDIRECT_MASK)
                                    .map_err(|error| {
                                        X11SetupSocketError::new(format!(
                                            "failed to inspect X11 configure redirect subscriptions: {error}"
                                        ))
                                    })?
                                    .iter()
                                    .any(|recipient| *recipient != client)
                                    .then_some(parent),
                                None => None,
                            }
                        }
                        _ => None,
                    };
                    // A size change on a window another client selected
                    // ResizeRedirect on reaches that client as a ResizeRequest
                    // and is not applied; the rest of the request still is.
                    const RESIZE_REDIRECT_MASK: u32 = 1 << 18;
                    let mut resize_request = None;
                    if redirected_configure.is_none()
                        && let (
                            Some(crate::XWireRequest::ConfigureWindow { window, value_mask, width, height, .. }),
                            Some(routing),
                        ) = (&configured, protocol_routing.as_ref())
                        && *value_mask & 0xC != 0
                        && let Ok(current) = runtime.window_geometry(namespace, *window)
                    {
                        let clamp = |value: i32| u16::try_from(value).unwrap_or(u16::MAX);
                        let (asked_width, asked_height) =
                            (width.unwrap_or(clamp(current.width)), height.unwrap_or(clamp(current.height)));
                        let changes = i32::from(asked_width) != current.width || i32::from(asked_height) != current.height;
                        let redirected = changes
                            && routing
                                .core_event_subscribers(*window, RESIZE_REDIRECT_MASK)
                                .map_err(|error| {
                                    X11SetupSocketError::new(format!(
                                        "failed to inspect X11 resize redirect subscriptions: {error}"
                                    ))
                                })?
                                .iter()
                                .any(|recipient| *recipient != client);
                        if redirected {
                            resize_request = Some(crate::XClientEvent::ResizeRequest {
                                sequence,
                                window: *window,
                                width: asked_width,
                                height: asked_height,
                            });
                            if let crate::XWireRequest::ConfigureWindow { value_mask, width, height, .. } = &mut request {
                                *value_mask &= !0xC;
                                *width = None;
                                *height = None;
                            }
                        }
                    }
                    // MapSubwindows under a managed parent: each unmapped,
                    // non-override-redirect child is the manager's to map, so
                    // the manager hears a MapRequest per child and those
                    // children stay unmapped; the rest map as before (XTS
                    // XMapSubwindows 5, t217).
                    let mut withheld_map_requests = Vec::new();
                    if let (crate::XWireRequest::MapSubwindows { window: parent, withheld }, Some(routing)) =
                        (&mut request, protocol_routing.as_ref())
                        && routing
                            .core_event_subscribers(*parent, SUBSTRUCTURE_REDIRECT_MASK)
                            .map_err(|error| {
                                X11SetupSocketError::new(format!(
                                    "failed to inspect X11 map-subwindows redirect subscriptions: {error}"
                                ))
                            })?
                            .iter()
                            .any(|recipient| *recipient != client)
                    {
                        let parent = *parent;
                        let children = runtime
                            .window_parent_and_children(namespace, parent)
                            .map(|(_, children)| children)
                            .unwrap_or_default();
                        for child in children {
                            if matches!(runtime.window_map_state(namespace, child), Ok(crate::XMapState::Unmapped))
                                && !runtime.window_override_redirect(namespace, child).unwrap_or(false)
                            {
                                withheld.push(child);
                                withheld_map_requests.push(crate::XClientEvent::MapRequest {
                                    sequence,
                                    parent,
                                    window: child,
                                });
                            }
                        }
                    }
                    // A circulate on a window another client manages
                    // (SubstructureRedirect selected on it) is that client's
                    // to decide: the child that would move is named in a
                    // CirculateRequest and nothing moves, as a map becomes a
                    // MapRequest.
                    let redirected_circulate = match (circulated, protocol_routing.as_ref()) {
                        (Some((parent, direction)), Some(routing)) => {
                            let redirected = routing
                                .core_event_subscribers(parent, SUBSTRUCTURE_REDIRECT_MASK)
                                .map_err(|error| {
                                    X11SetupSocketError::new(format!(
                                        "failed to inspect X11 circulate redirect subscriptions: {error}"
                                    ))
                                })?
                                .iter()
                                .any(|recipient| *recipient != client);
                            if redirected {
                                Some((parent, direction, runtime.circulate_candidate(namespace, parent, direction)))
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    let private_focus_routing = requested_input_focus.and(protocol_routing.as_ref())
                        .filter(|routing| routing.private_applied.get().is_some());
                    let mut output = match explicit_pointer_preparation {
                        _ if redirected_circulate.is_some() => {
                            runtime.begin_dispatch();
                            let (parent, place, candidate) = redirected_circulate.expect("redirect guard");
                            let outputs = match candidate {
                                Ok(Some(window)) => vec![crate::XClientOutput::Event(
                                    crate::XClientEvent::CirculateRequest {
                                        sequence,
                                        parent,
                                        window,
                                        place,
                                    },
                                )],
                                Ok(None) => Vec::new(),
                                Err(error) => vec![crate::XClientOutput::Error(crate::x_error_from_runtime(
                                    error,
                                    sequence,
                                    major_opcode,
                                    0,
                                    u32::try_from(parent.local.raw()).unwrap_or(0),
                                ))],
                            };
                            XDispatchResult {
                                response: None,
                                outputs,
                                metadata_candidates: Vec::new(),
                            }
                        }
                        _ if redirected_configure.is_some() => {
                            runtime.begin_dispatch();
                            let parent = redirected_configure.expect("redirect guard");
                            let Some(crate::XWireRequest::ConfigureWindow {
                                window, value_mask, x, y, width, height, border_width, sibling, stack_mode,
                            }) = configured
                            else {
                                unreachable!("a configure redirect names a ConfigureWindow")
                            };
                            let current = runtime.window_geometry(namespace, window).unwrap_or_default();
                            let clamp_i = |value: i32| i16::try_from(value).unwrap_or(i16::MAX);
                            let clamp_u = |value: i32| u16::try_from(value).unwrap_or(u16::MAX);
                            XDispatchResult {
                                response: None,
                                outputs: vec![crate::XClientOutput::Event(
                                    crate::XClientEvent::ConfigureRequest {
                                        sequence,
                                        stack_mode: stack_mode.unwrap_or(0),
                                        parent,
                                        window,
                                        sibling: sibling.unwrap_or(crate::XResourceId::NONE),
                                        x: x.unwrap_or(clamp_i(current.x)),
                                        y: y.unwrap_or(clamp_i(current.y)),
                                        width: width.unwrap_or(clamp_u(current.width)),
                                        height: height.unwrap_or(clamp_u(current.height)),
                                        border_width: border_width.unwrap_or_else(|| runtime.window_border_width(window)),
                                        value_mask,
                                    },
                                )],
                                metadata_candidates: Vec::new(),
                            }
                        }
                        _ if redirected_map.is_some() => {
                            runtime.begin_dispatch();
                            let (parent, window) = redirected_map.expect("redirect guard");
                            // Emitted as an ordinary lifecycle event addressed
                            // to the parent. The existing routing delivers it
                            // to whoever selected on that parent and drops it
                            // from this client's own stream unless it selected
                            // too, which is the same path every other
                            // substructure event takes.
                            XDispatchResult {
                                response: None,
                                outputs: vec![crate::XClientOutput::Event(
                                    crate::XClientEvent::MapRequest {
                                        sequence,
                                        parent,
                                        window,
                                    },
                                )],
                                metadata_candidates: Vec::new(),
                            }
                        }
                        _ if private_focus_routing.is_some() => {
                            runtime.begin_dispatch();
                            let (focus, revert_to, focus_time) = requested_input_focus.expect("focus guard");
                            let (output, pending) = x11_dispatch_private_focus(&mut runtime, dispatch_context, client,
                                &focused_surface_window, private_focus_routing.expect("private owner"), focus, revert_to, focus_time,
                                state.runtime.clone(), state.control_runtime_pending.clone(), output_stream.clone(), output_control_pending.clone(), output_wire.clone())?;
                            // Behaviour behind `_NET_ACTIVE_WINDOW`, on the path a client's own
                            // SetInputFocus takes when a private focus claim is in play.
                            crate::dispatch::publish_noted_focus(&mut runtime, &mut properties, &mut atoms, dispatch_context.byte_order);
                            pending_focus_publication = pending;
                            output
                        }
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
                    if let Some(event) = resize_request {
                        output.outputs.push(crate::XClientOutput::Event(event));
                    }
                    output
                        .outputs
                        .extend(withheld_map_requests.into_iter().map(crate::XClientOutput::Event));
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
                                } else if let Err(refusal) =
                                    routing.route_protocol_to_watcher(recipient, event)
                                {
                                    match refusal {
                                        // ENDED BY THE IDENTITY THAT STALLED. The
                                        // number alone could by now be a successor's.
                                        XServerFrontendWatcherRefusal::Stalled(stalled) => {
                                            let watcher = stalled.client();
                                            routing
                                                .disconnect_saturated_recipient(stalled)
                                                .map_err(|error| {
                                                    X11SetupSocketError::new(format!(
                                                        "failed to end a stalled XFixes watcher {}: {error}",
                                                        watcher.raw()
                                                    ))
                                                })?;
                                        }
                                        XServerFrontendWatcherRefusal::Route(error) => {
                                            if !x11_recipient_is_gone(&error) {
                                                return Err(X11SetupSocketError::new(format!(
                                                    "failed to route an XFixes selection change: {error}"
                                                )));
                                            }
                                        }
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
                        if requested_input_focus.is_some() && private_focus_routing.is_none() {
                            // A request the protocol discarded for its timestamp,
                            // or refused outright, leaves the focus where it was.
                            // The projection follows what the runtime actually
                            // holds rather than what the client asked for.
                            focused_surface_window.store(
                                runtime.input_focus(namespace).0.local.raw(),
                                Ordering::Release,
                            );
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
                            selections.select_xkb_state_notifications(
                                &xkb_state_details, affect_which, clear, select_all, state,
                            );
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
            // An accepted XTEST request, acted on now that the guard which
            // validated it is released. Acceptance is the absence of an error:
            // FakeInput and GrabControl owe no reply, so an empty output set is
            // the dispatcher saying yes.
            if output.outputs.is_empty() {
                if let (Some(connection), Some(impervious)) = (xtest.as_mut(), grab_control) {
                    connection.impervious = impervious == 1;
                }
                match lifetime_request.take() {
                    Some(crate::XWireRequest::SetCloseDownMode { mode }) => {
                        state.set_close_down_mode(client, mode)?;
                    }
                    Some(crate::XWireRequest::ChangeSaveSet { window, mode, .. }) => {
                        state.change_save_set(client, window, mode)?;
                    }
                    Some(crate::XWireRequest::KillClient { resource }) => {
                        let ended_self = apply_x11_kill_client(
                            state,
                            protocol_routing.as_ref(),
                            client,
                            resource,
                            transaction,
                            sequence,
                            major_opcode,
                            &mut output,
                            &mut released_dma_bufs,
                            &mut released_fences,
                        )?;
                        if ended_self {
                            return Ok(());
                        }
                    }
                    _ => {}
                }
                // An accepted WarpPointer has placed the position the
                // dispatcher keeps; a client that may inject input moves the
                // routed pointer there too, as an absolute motion, so the
                // crossing and motion events a warp owes are the ones any
                // motion owes. Without an injector the warp stays what it
                // was: a position QueryPointer reports (the pointer is the
                // Engine's, and a client that may not inject may not move it).
                if warp_pointer && xtest.is_some() {
                    let runtime = lock_x11_request_runtime(
                        &state.runtime,
                        &state.control_runtime_pending,
                    )?;
                    fake_input = runtime
                        .pointer_query_position(namespace)
                        .filter(|placed| Some(*placed) != pointer_before_warp)
                        .map(|(root_x, root_y)| XTestFakeInputRequest::absolute_motion(root_x, root_y));
                }
                if let (Some(connection), Some(request)) = (xtest.as_mut(), fake_input) {
                    let planned = {
                        let runtime = lock_x11_request_runtime(
                            &state.runtime,
                            &state.control_runtime_pending,
                        )?;
                        connection.plan(&runtime, namespace, request)
                    };
                    // Nothing here reaches the client. FakeInput has no reply
                    // and the reference sends none, so an error synthesised
                    // for a refusal would desynchronise its sequence
                    // accounting. What the client gets instead is that its
                    // next request is not read until this one has been
                    // processed, which is the barrier below.
                    match planned {
                        Ok(plan) => match connection.submit_and_await(stream, plan)? {
                            XTestWaitEnd::Departed => return Ok(()),
                            XTestWaitEnd::Settled | XTestWaitEnd::Cancelled => {}
                        },
                        Err(_unplanned) => {}
                    }
                }
            }
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
            crate::evidence::window_dispatch(
                client, sequence, major_opcode,
                (major_opcode == 1 && request.len() >= 32)
                    .then(|| setup.byte_order.u16(&request[22..24])),
                output,
            );
            let encoded_outputs = output.encoded_outputs(setup.byte_order);
            let receipt = observer(pending_observation.take().expect("one observation per allocated ticket"))?;
            if let Some(receipt) = receipt { last_published_observation = Some(receipt); }
            if let Some(pending) = pending_focus_publication.as_mut() {
                if !server_reply_fds.is_empty() {
                    return Err(X11SetupSocketError::new("private core focus cannot carry descriptor replies"));
                }
                pending.records = Some(encoded_outputs);
                pending.write_output(&event_sequence, sequence)?;
                pending_focus_publication = None;
            } else {
                // No stop flag: this is the dispatch thread itself, and it is
                // the thread that will later stop and join the writers. There
                // is nothing for it to observe being told by, so the wait
                // stays uncancellable here and is bounded instead by control
                // output finishing.
                let mut output_stream = lock_x11_non_control_output(
                    &output_stream,
                    &output_wire,
                    &output_control_pending,
                    None,
                )?
                .expect("an uncancellable wait yields the socket");
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
            }
            for delivery in peer_msc_deliveries {
                protocol_routing
                    .as_ref()
                    .expect("MSC subscription has a route registry")
                    .route_protocol_contained(delivery.recipient, delivery.event)
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
        result.is_ok(),
    ) {
        let published = observer(observation).map(|_| ());
        if dispatch_started && !dispatch_complete {
            published.and(Err(partial_dispatch_error(result.is_ok())))
        } else {
            published
        }
    } else {
        Ok(())
    };
    // Every writer is stopped before any is joined, and every one is joined
    // whatever the others did. Returning on the first failure left the rest
    // running, never told to stop, against a stream about to close.
    let writer_result = writers.shut_down().outcome;
    owned.query_owner.finish()?;
    if let Some(routing) = protocol_routing.as_ref() {
        let mut pointers = routing.pointer_state.lock()
            .map_err(|_| X11SetupSocketError::new("X11 pointer state lock poisoned"))?;
        let authority = routing.input_authority.lock()
            .map_err(|_| X11SetupSocketError::new("X11 input authority lock poisoned"))?;
        if routing.input_recovery.lifecycle.get().is_none() && !authority.query_namespace_active(namespace) {
            pointers.retain(|(owner, _), _| *owner != namespace);
        }
    }
    state.runtime.lock()
        .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
        .release_client_device_bundle(client.raw());
    let client_lease = state.release_client(client)?;
    debug_assert_eq!(client_lease.resource_id_range, resource_id_range);
    let save_set = state.take_save_set(client)?.into_iter().collect::<Vec<_>>();
    // A retain mode keeps the range -- windows mapped, properties in place --
    // until a KillClient names one of its resources (or AllTemporary, when
    // temporary). Selections end with the connection either way, as the
    // reference server ends them; the save-set is honoured either way too.
    let retained = match client_lease.close_down_mode {
        crate::XCloseDownMode::Destroy => None,
        crate::XCloseDownMode::RetainPermanent => Some(false),
        crate::XCloseDownMode::RetainTemporary => Some(true),
    };
    let mut release = match retained {
        None => {
            if let Some(source) = &control_cleanup_source {
                release_x11_client_lease_with_control(state, namespace, client_lease, &save_set, Some(source))?
            } else {
                release_x11_client_lease_with_control(state, namespace, client_lease, &save_set, None)?
            }
        }
        Some(temporary) => {
            let release = {
                let mut runtime = state.runtime.lock()
                    .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?;
                runtime.retain_client_resource_range(namespace, resource_id_range, &save_set)
                    .map_err(|error| X11SetupSocketError::new(format!("failed to retain X11 client resources: {error:?}")))?
            };
            if let Some(source) = &control_cleanup_source {
                source.record_removal(state, &client_lease, &release)?;
            }
            state.retain_client_range(XRetainedClientRange {
                client,
                namespace,
                range: resource_id_range,
                temporary,
            })?;
            release
        }
    };
    release.released_dma_bufs.extend(state.runtime.lock()
        .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
        .take_retired_pixmap_registrations(namespace));
    release.released_dma_bufs.sort_unstable();
    release.released_dma_bufs.dedup();
    if let Some(source) = &control_cleanup_source {
        source.teardown.lock().map_err(|_| X11SetupSocketError::new("control teardown unavailable"))?
            .removed.as_mut().ok_or_else(|| X11SetupSocketError::new("control removal receipt missing"))?.resources = release.clone();
    }
    // Ownerships this client took with another client's window end with it
    // too: ownership is the requester's, not the window's (t226).
    release.retired_selection_ownerships.extend(
        state
            .runtime
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
            .retire_selections_requested_by(client.raw()),
    );
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
                if let Err(refusal) = routing.route_protocol_to_watcher(
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
                    match refusal {
                        // ENDED BY THE IDENTITY THAT STALLED. The number alone
                        // could by now be a successor's.
                        XServerFrontendWatcherRefusal::Stalled(stalled) => {
                            let watcher = stalled.client();
                            routing
                                .disconnect_saturated_recipient(stalled)
                                .map_err(|error| {
                                    X11SetupSocketError::new(format!(
                                        "failed to end a stalled XFixes watcher {}: {error}",
                                        watcher.raw()
                                    ))
                                })?;
                        }
                        XServerFrontendWatcherRefusal::Route(error) => {
                            if !x11_recipient_is_gone(&error) {
                                return Err(X11SetupSocketError::new(format!(
                                    "failed to route a departed peer's selection change: {error}"
                                )));
                            }
                        }
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
        route_x11_save_set_reparents(routing, &release.save_set_reparents)?;
    }
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
                        .route_protocol_contained(
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
    // AFTER THE WRITERS ARE JOINED, AND THAT ORDER IS LOAD-BEARING. The input
    // writer's recovery guard disconnects this client BY NUMBER when its
    // thread ends. `writers.shut_down()` above stopped and joined every
    // writer synchronously, so that action has completed here, while this
    // registration -- and with it the number's claim -- is still alive; it
    // can therefore reach only this connection's ledger entry. Every early
    // return and unwind between the writers' spawn and here keeps the same
    // order for a different reason: `owned` is declared after
    // `route_registration`, so it is dropped first. Nothing enforces either
    // by type. A registration dropped before its writers are joined would
    // release the number under a by-number act still to come.
    drop(route_registration);
    // STAGE-ONLY SCHEDULING HOOK, TEST BUILDS ONLY: the interval after this
    // connection's registration has gone and before the rest of its frame --
    // the disconnect observer, revocation, completion -- has run. A control
    // that must show that a registration's destruction is not the frame's
    // completion pauses the frame here; production builds compile nothing.
    #[cfg(all(test, unix))]
    routing_tests::stage_after_registration_drop(protocol_routing.as_ref(), client);
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
        
        let observation = X11DispatchObservation {
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
        };
        if let Some(source) = &control_cleanup_source {
            source.retain_teardown_publication(&observation)?;
        }
        observer(observation).map(|_| ())
    };
    if cleanup_observer_result.is_ok() && let Some(source) = &control_cleanup_source {
        source.finish_teardown()?;
    }
    let admission_result = admission_lease.as_mut().map_or(Ok(()), |lease| {
        lease.revoke().map_err(|error| {
            X11SetupSocketError::new(format!("failed to revoke X11 client admission: {error}"))
        })
    });
    // THE PLACE THIS CONNECTION LEFT GOES BACK DURING THE RUN.
    //
    // A continuation place is held from reservation until the work left in it
    // is gone, and the only thing that notices it is gone is a visit. Every
    // other reclamation in this instance is hung off the maintenance keeper,
    // which runs a bounded number of visits AFTER the invocation has ended --
    // so a place handed over here stayed taken for the whole remaining life of
    // the service, and an instance that had seen `max_concurrent_clients`
    // departures could admit nobody.
    //
    // Driven here, on the way out and by the connection that is leaving, so
    // the reclamation is live. Unconditional: whatever this connection's
    // result turns out to be below, it is leaving and what it left is still
    // owed a visit. This does not reclaim this connection's own place -- its
    // work may not have settled yet, and a place comes back only when the work
    // in it is gone -- it reclaims whoever settled since the last departure.
    // The admission path drives again before refusing, which is what covers
    // the place that settles after this.
    if let Some(routing) = protocol_routing.as_ref() {
        routing.drive_departed_continuations();
    }
    pending_publication_result?;
    result?;
    writer_result?;
    cleanup_observer_result?;
    admission_result
}
