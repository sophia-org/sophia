// Applying a control, and everything that belongs to doing so exactly once.
//
// Split from the other writers by subject rather than by size: this one is the
// only writer that applies commands to the runtime, so what may be applied,
// what stops it, and what answers for it afterwards are all here, while the
// writers that only serialise events and the thing that owns all three stay
// next door.

/// Records that a client's control writer has stopped.
///
/// Every way out of the loop below is a cancellation edge: a stop flag, a
/// disconnected route queue, a terminated client, a failure partway through an
/// operation, a full acknowledgement channel, or an unwind. A guard rather
/// than a call at the end, because the last of those reaches no call at the
/// end, and a client whose writer has gone must stop having work accepted for
/// it however it went.
///
/// The state lives with the client's route senders, because a registration
/// outlives its writer -- returning on a full channel is exactly that -- so
/// the registration being alive is not evidence that anything is left to
/// execute.
///
/// Registrations themselves stay where they are. This writer does not own the
/// queue those commands came from and cannot decide their fate without
/// discarding them.
#[cfg(unix)]
struct X11ControlWriterSeal<'a> {
    routing: Option<&'a XServerFrontendRouteRegistry>,
    client: XServerFrontendClientId,
}

#[cfg(unix)]
impl Drop for X11ControlWriterSeal<'_> {
    fn drop(&mut self) {
        let Some(routing) = self.routing else {
            return;
        };
        routing.mark_control_writer_gone(self.client);
        // This writer stops being an executor before anything asks whether one
        // is left. It is not the whole executor: routing produces authoritative
        // effects before any writer runs, so a routing call still in flight
        // keeps this client executing and nothing of its is abandoned.
        if let Some(completion) = routing.control_completion() {
            // This writer stops being an executor before anything asks whether
            // one is left, and it is not the whole executor: routing produces
            // authoritative effects before any writer runs, so a routing call
            // still in flight keeps this client executing and nothing of its
            // is abandoned.
            completion.writer_stopped(self.client);
        }
    }
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn spawn_x11_control_writer(
    stream: Arc<Mutex<UnixStream>>,
    output_control_pending: Arc<AtomicUsize>,
    output_wire: Arc<X11WirePermission>,
    byte_order: XByteOrder,
    sequence: Arc<AtomicU16>,
    focused_surface_window: Arc<AtomicU64>,
    surface_windows: Arc<Mutex<BTreeMap<SurfaceId, XResourceId>>>,
    metadata_rules: Arc<Mutex<BTreeMap<SurfaceId, MetadataDisclosureRule>>>,
    metadata_generations: Arc<Mutex<BTreeMap<SurfaceId, u64>>>,
    core_event_selections: Arc<Mutex<XCoreEventSelectionState>>,
    xkb_modifiers: Arc<AtomicU16>,
    atoms: Arc<Mutex<XAtomTable>>,
    properties: Arc<Mutex<XPropertyTable>>,
    runtime: Arc<Mutex<XAuthorityRuntime>>,
    control_runtime_pending: Arc<AtomicUsize>,
    resource_id_range: crate::XWireClientResourceRange,
    namespace: NamespaceId,
    client: XServerFrontendClientId,
    protocol_routing: Option<XServerFrontendRouteRegistry>,
    channels: X11ControlChannels,
) -> Result<X11ControlWriter, X11SetupSocketError> {
    let stop = Arc::new(AtomicBool::new(false));
    let writer_stop = stop.clone();
    macro_rules! terminate_client {
        ($kind:expr, $transaction:expr, $surface:expr, $completion:expr) => {{
            let stream = stream
                .lock()
                .map_err(|_| X11SetupSocketError::new("X11 output socket lock poisoned"))?;
            stream.shutdown(Shutdown::Both).map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to terminate non-cooperating X11 client: {error}"
                ))
            })?;
            drop(stream);
            // The shutdown is the effect, so this acknowledgement is the
            // operation's real outcome and closes its record.
            channels.send_ack_for(
                client,
                XAuthorityControlAck {
                    kind: $kind,
                    transaction: $transaction,
                    surface: $surface,
                    outcome: XAuthorityControlOutcome::Delivered,
                },
                $completion,
            )?;
            return Ok(());
        }};
    }
    let thread = std::thread::spawn(move || {
        if let Some(completion) = protocol_routing
            .as_ref()
            .and_then(XServerFrontendRouteRegistry::control_completion)
        {
            completion.writer_started(client);
        }
        let _seal = X11ControlWriterSeal {
            routing: protocol_routing.as_ref(),
            client,
        };
        let run = || -> Result<(), X11SetupSocketError> {
        while !writer_stop.load(Ordering::Acquire) {
            let routed = match channels.recv_timeout(client) {
                Ok(routed) => routed,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            };
            // Once a control leaves its route queue, no ordinary reply or
            // event may repeatedly overtake the write that makes it visible.
            let _output_priority =
                X11ControlOutputPriority::new(output_control_pending.clone());
            // Marked before anything is written, because everything after
            // this point can leave the runtime changed with no acknowledgement
            // sent.
            let (command, focus_transition, completion, focus_claim) = match routed {
                X11RoutedControl::Authority {
                    command,
                    focus,
                    completion,
                    claim,
                } => (command, focus, completion, claim),
                X11RoutedControl::FocusOut {
                    window,
                    time_msec,
                    origin,
                    claim,
                } => {
                    let disposition = x11_apply_dependent_focus_out(
                        namespace, client, window, &focused_surface_window,
                        protocol_routing.as_ref(), claim.as_ref(),
                    ).map_err(|cause| X11SetupSocketError::new(format!("private FocusOut unavailable: {cause:?}")))?;
                    if disposition != X11DependentFocusEffect::ProjectionCleared {
                        // A stale generationless FocusOut would undo the
                        // newer FocusIn at the client even if our atomic were
                        // preserved. End only this dependency's quiescence.
                        tracing::debug!(?disposition, "private FocusOut not applicable");
                        drop(origin);
                        continue;
                    }
                    let records = x11_focus_records(
                        byte_order,
                        sequence.load(Ordering::Acquire),
                        namespace,
                        client,
                        &core_event_selections,
                        protocol_routing
                            .as_ref()
                            .map(|routing| &routing.input_authority),
                        xkb_modifiers.load(Ordering::Acquire),
                        X11FocusRecordRequest::Event {
                            window,
                            focused: false,
                            time_msec,
                        },
                    )?;
                    write_x11_control_records(
                        &stream,
                        &output_wire,
                        byte_order,
                        &sequence,
                        records,
                    )?;
                    // Run, so it can no longer happen, and its origin is told
                    // by the same guard that would have told it had this queue
                    // gone instead. Not an outcome for that operation: only
                    // that this particular effect of it is over.
                    drop(origin);
                    continue;
                }
            };
            // Before anything that answers for this operation, including the
            // refusals below: an acknowledgement is an outcome, and producing
            // one for work whose outcome belongs to another owner is the same
            // error as applying it. A refusal here leaves that owner holding
            // it -- the record if it is still held, whoever took it if it is
            // not -- so nothing is dropped by declining.
            if !channels.resume_execution(completion).permits_effects() {
                continue;
            }

            let transaction = command.transaction();
            let surface = command.surface();
            let kind = command.kind();
            let window = surface_windows
                .lock()
                .map_err(|_| X11SetupSocketError::new("X11 surface/window map lock poisoned"))?
                .get(&surface)
                .copied();
            let Some(window) = window else {
                channels.send_ack_for(
                    client,
                    XAuthorityControlAck {
                        kind,
                        transaction,
                        surface,
                        outcome: XAuthorityControlOutcome::UnknownSurface,
                    },
                    completion,
                )?;
                continue;
            };

            let event_sequence = sequence.load(Ordering::Acquire);
            let records = match command {
                XAuthorityControlCommand::PublishMetadataRule { rule, .. } => {
                    if rule.surface != surface {
                        channels.send_ack_for(
                            client,
                            XAuthorityControlAck {
                                kind,
                                transaction,
                                surface,
                                outcome: XAuthorityControlOutcome::AuthorityRejected,
                            },
                            completion,
                        )?;
                        continue;
                    }
                    let atoms = atoms
                        .lock()
                        .map_err(|_| X11SetupSocketError::new("X11 atom table lock poisoned"))?;
                    let properties = properties.lock().map_err(|_| {
                        X11SetupSocketError::new("X11 property table lock poisoned")
                    })?;
                    metadata_rules
                        .lock()
                        .map_err(|_| {
                            X11SetupSocketError::new("X11 metadata rule lock poisoned")
                        })?
                        .insert(surface, rule);
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
                    drop(properties);
                    drop(atoms);
                    if let Some(routing) = protocol_routing.as_ref() {
                        routing.emit_metadata_candidate(candidate).map_err(|error| {
                            X11SetupSocketError::client_failure(format!(
                                "failed to publish reduced X11 metadata: {error:?}"
                            ))
                        })?;
                    }
                    Vec::new()
                }
                XAuthorityControlCommand::AdmitSurface { geometry, .. } => {
                    let geometry = match lock_x11_control_runtime(
                        &runtime,
                        &control_runtime_pending,
                    )?
                        .admit_window_from_engine(namespace, window, geometry)
                    {
                        Ok(geometry) => geometry,
                        Err(_) => {
                            channels.send_ack_for(
                                client,
                                XAuthorityControlAck {
                                    kind,
                                    transaction,
                                    surface,
                                    outcome: XAuthorityControlOutcome::AuthorityRejected,
                                },
                                completion,
                            )?;
                            continue;
                        }
                    };
                    let mut selections = core_event_selections
                        .lock()
                        .map_err(|_| {
                            X11SetupSocketError::new("X11 core event selection lock poisoned")
                        })?;
                    selections.update_geometry(window, geometry);
                    let map_transition = selections.observe_mapped(window);
                    if std::env::var_os("SOPHIA_LIVE_SESSION_DIAGNOSTIC").is_some() {
                        tracing::debug!(
                            "sophia_x11_viewability schema=1 status=admitted viewable={} promoted_descendants={}",
                            map_transition.viewable,
                            map_transition.promoted_descendants.len(),
                        );
                    }
                    x11_surface_geometry_records(
                        byte_order,
                        event_sequence,
                        client,
                        window,
                        geometry,
                        true,
                        Some(&map_transition),
                        true,
                        &selections,
                        protocol_routing.as_ref(),
                    )?
                }
                XAuthorityControlCommand::ConfigureSurface { geometry, .. } => {
                    if geometry.is_empty()
                        || geometry.width > i32::from(u16::MAX)
                        || geometry.height > i32::from(u16::MAX)
                        || geometry.x < i32::from(i16::MIN)
                        || geometry.x > i32::from(i16::MAX)
                        || geometry.y < i32::from(i16::MIN)
                        || geometry.y > i32::from(i16::MAX)
                    {
                        channels.send_ack_for(
                            client,
                            XAuthorityControlAck {
                                kind,
                                transaction,
                                surface,
                                outcome: XAuthorityControlOutcome::InvalidSize,
                            },
                            completion,
                        )?;
                        continue;
                    }
                    // Recorded before the change can happen, and the change
                    // does not happen if it cannot be recorded: an effect
                    // nobody noted the intent for cannot afterwards be told
                    // from one that never happened.
                    channels
                        .record_progress(completion, ControlProgress::RuntimeBegun)
                        .map_err(|refusal| {
                            X11SetupSocketError::new(format!(
                                "X11 control could not record that it was about to change the \
                                 runtime: {refusal:?}"
                            ))
                        })?;
                    let mut runtime =
                        lock_x11_control_runtime(&runtime, &control_runtime_pending)?;
                    let previous_geometry = runtime.window_geometry(namespace, window).ok();
                    let geometry = match runtime.configure_window_from_engine(
                        namespace,
                        window,
                        geometry,
                    ) {
                        Ok(geometry) => geometry,
                        Err(_) => {
                            channels.send_ack_for(
                                client,
                                XAuthorityControlAck {
                                    kind,
                                    transaction,
                                    surface,
                                    outcome: XAuthorityControlOutcome::AuthorityRejected,
                                },
                                completion,
                            )?;
                            continue;
                        }
                    };
                    // Reported by the code that did it, immediately after it
                    // succeeded. From here to the projection below is the
                    // window where this operation has changed shared state
                    // that outlives the connection and the state derived from
                    // it does not agree yet.
                    drop(runtime);
                    // Reported after the guard goes. Not because the pair
                    // would be a cycle -- every refusal arm here already
                    // acknowledges with its runtime guard live, and that edge
                    // runs into the registry like all the others -- but
                    // because there is no reason to hold the runtime for it.
                    // Between the change and this the step reads as in
                    // progress, which is the safe reading: it says the effect
                    // may have happened, and nothing is discharged on it.
                    let _ = channels.record_progress(completion, ControlProgress::RuntimeApplied);
                    channels
                        .record_progress(completion, ControlProgress::ProjectionBegun)
                        .map_err(|refusal| {
                            X11SetupSocketError::new(format!(
                                "X11 control could not record that it was about to project its \
                                 runtime change: {refusal:?}"
                            ))
                        })?;
                    let mut selections = core_event_selections
                        .lock()
                        .map_err(|_| {
                            X11SetupSocketError::new("X11 core event selection lock poisoned")
                        })?;
                    selections.update_geometry(window, geometry);
                    // Reported while the selections guard is held, because the
                    // records below need it. Lock rank: selections are taken
                    // before the completion registry, as the runtime and the
                    // atom and property tables are elsewhere in this file. All
                    // of those edges point into the registry, and what the
                    // registry does under its own lock is one thing -- a
                    // non-blocking enqueue on the acknowledgement channel --
                    // so none of them runs the other way.
                    let _ =
                        channels.record_progress(completion, ControlProgress::ProjectionApplied);
                    if previous_geometry == Some(geometry) {
                        Vec::new()
                    } else {
                        // XLibre's Present hook runs before core event
                        // delivery for every real geometry change, including
                        // a pure move. Clients may merge both streams.
                        x11_surface_geometry_records(
                            byte_order,
                            event_sequence,
                            client,
                            window,
                            geometry,
                            false,
                            None,
                            true,
                            &selections,
                            protocol_routing.as_ref(),
                        )?
                    }
                }
                XAuthorityControlCommand::SetPresentationState { state, .. }
                | XAuthorityControlCommand::RestorePresentationState { state, .. } => {
                    let mut atoms = atoms
                        .lock()
                        .map_err(|_| X11SetupSocketError::new("X11 atom table lock poisoned"))?;
                    let mut properties = properties.lock().map_err(|_| {
                        X11SetupSocketError::new("X11 property table lock poisoned")
                    })?;
                    let changed = match apply_engine_presentation_state(
                        &mut properties,
                        &mut atoms,
                        namespace,
                        window,
                        byte_order,
                        state,
                    ) {
                        Ok(changed) => changed,
                        Err(_) => {
                            channels.send_ack_for(
                                client,
                                XAuthorityControlAck {
                                    kind,
                                    transaction,
                                    surface,
                                    outcome: XAuthorityControlOutcome::AuthorityRejected,
                                },
                                completion,
                            )?;
                            continue;
                        }
                    };
                    drop(properties);
                    drop(atoms);
                    let selections = core_event_selections.lock().map_err(|_| {
                        X11SetupSocketError::new("X11 core event selection lock poisoned")
                    })?;
                    x11_presentation_property_records(
                        byte_order,
                        event_sequence,
                        client,
                        window,
                        &changed,
                        &selections,
                        protocol_routing.as_ref(),
                    )?
                }
                XAuthorityControlCommand::CloseSurface { .. } => {
                    let atoms = atoms
                        .lock()
                        .map_err(|_| X11SetupSocketError::new("X11 atom table lock poisoned"))?;
                    let Some(protocols) = atoms.atom(X_ATOM_NAME_WM_PROTOCOLS) else {
                        terminate_client!(kind, transaction, surface, completion);
                    };
                    let Some(delete) = atoms.atom(X_ATOM_NAME_WM_DELETE_WINDOW) else {
                        terminate_client!(kind, transaction, surface, completion);
                    };
                    drop(atoms);
                    let properties = properties.lock().map_err(|_| {
                        X11SetupSocketError::new("X11 property table lock poisoned")
                    })?;
                    let protocol_windows = properties.windows_with_property(namespace, protocols);
                    let advertises_delete = |candidate: &XResourceId| {
                        u32::try_from(candidate.local.raw())
                            .is_ok_and(|raw| resource_id_range.owns_new_resource(raw))
                            && properties
                                .get(namespace, *candidate, protocols)
                                .is_some_and(|record| {
                                    record.format == 32
                                        && record
                                            .bytes
                                            .chunks_exact(4)
                                            .any(|bytes| byte_order.u32(bytes) == delete)
                                })
                    };
                    let candidates: Vec<_> = protocol_windows
                        .iter()
                        .map(|candidate| (*candidate, advertises_delete(candidate)))
                        .collect();
                    let ancestors = core_event_selections
                        .lock()
                        .map_err(|_| {
                            X11SetupSocketError::new("X11 core event selection lock poisoned")
                        })?
                        .ancestors(window);
                    let decision = crate::select_x_close_target(window, &ancestors, &candidates);
                    if decision.protocol_window_count == 0 {
                        drop(properties);
                        terminate_client!(kind, transaction, surface, completion);
                    }
                    tracing::debug!(
                        "sophia_x11_close_target schema=1 surface_map_hit=true exact_delete={} fallback_used={} protocol_windows={}",
                        decision.exact_advertises_delete,
                        decision.fallback_used,
                        decision.protocol_window_count,
                    );
                    let window = decision.window;
                    let mut bytes = [0_u8; 32];
                    // ICCCM WM_DELETE_WINDOW is delivered via SendEvent, so
                    // the synthetic-event bit must be set on ClientMessage.
                    bytes[0] = 33 | 0x80;
                    bytes[1] = 32;
                    write_control_u32(byte_order, &mut bytes[4..8], window.local.raw() as u32);
                    write_control_u32(byte_order, &mut bytes[8..12], protocols);
                    write_control_u32(byte_order, &mut bytes[12..16], delete);
                    vec![encode_x_client_event(
                        byte_order,
                        XClientEvent::ClientMessage {
                            sequence: event_sequence,
                            bytes,
                        },
                    )]
                }
                XAuthorityControlCommand::FocusSurface { .. } => {
                    let applied = {
                        let mut runtime = lock_x11_control_runtime(&runtime, &control_runtime_pending)?;
                        x11_apply_focus_change(&mut runtime, namespace, client, &focused_surface_window,
                            protocol_routing.as_ref(), focus_claim.as_ref(), X11FocusChange::Surface { window })
                    };
                    let applied = match applied {
                        Ok(applied) => applied,
                        Err(X11FocusApplyError::Runtime(_) | X11FocusApplyError::Superseded
                            | X11FocusApplyError::State(PrivateAppliedRegistryRefusal::MissingAdmission
                                | PrivateAppliedRegistryRefusal::AdmissionClosed | PrivateAppliedRegistryRefusal::ForeignOrigin)) => {
                            channels.send_ack_for(client, XAuthorityControlAck {
                                kind, transaction, surface, outcome: XAuthorityControlOutcome::AuthorityRejected,
                            }, completion)?;
                            continue;
                        }
                        Err(cause) => return Err(X11SetupSocketError::new(format!("private focus unavailable: {cause:?}"))),
                    };
                    let previous = applied.previous_authority;
                    let previous_routed = applied.previous_routed;
                    x11_focus_records(
                        byte_order,
                        event_sequence,
                        namespace,
                        client,
                        &core_event_selections,
                        protocol_routing
                            .as_ref()
                            .map(|routing| &routing.input_authority),
                        xkb_modifiers.load(Ordering::Acquire),
                        X11FocusRecordRequest::Surface {
                            window,
                            previous_authority: previous,
                            previous_routed,
                            transition: focus_transition,
                        },
                    )?
                }
                XAuthorityControlCommand::ClearFocus { .. } => {
                    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
                    let applied = {
                        let mut runtime = lock_x11_control_runtime(&runtime, &control_runtime_pending)?;
                        x11_apply_focus_change(&mut runtime, namespace, client, &focused_surface_window,
                            protocol_routing.as_ref(), focus_claim.as_ref(), X11FocusChange::Clear)
                    };
                    let applied = match applied {
                        Ok(applied) => applied,
                        Err(X11FocusApplyError::Runtime(_) | X11FocusApplyError::Superseded
                            | X11FocusApplyError::State(PrivateAppliedRegistryRefusal::MissingAdmission
                                | PrivateAppliedRegistryRefusal::AdmissionClosed | PrivateAppliedRegistryRefusal::ForeignOrigin)) => {
                            channels.send_ack_for(client, XAuthorityControlAck {
                                kind, transaction, surface, outcome: XAuthorityControlOutcome::AuthorityRejected,
                            }, completion)?;
                            continue;
                        }
                        Err(cause) => return Err(X11SetupSocketError::new(format!("private focus unavailable: {cause:?}"))),
                    };
                    let previous_routed = applied.previous_routed;
                    x11_focus_records(
                        byte_order,
                        event_sequence,
                        namespace,
                        client,
                        &core_event_selections,
                        protocol_routing
                            .as_ref()
                            .map(|routing| &routing.input_authority),
                        xkb_modifiers.load(Ordering::Acquire),
                        X11FocusRecordRequest::Clear {
                            root,
                            previous_routed,
                            transition: focus_transition,
                        },
                    )?
                }
                XAuthorityControlCommand::WithdrawSurface { .. } => {
                    let was_active = match lock_x11_control_runtime(
                        &runtime,
                        &control_runtime_pending,
                    )?
                        .unmap_window(namespace, window)
                    {
                        Ok(surface) => surface.is_some(),
                        Err(_) => {
                            channels.send_ack_for(
                                client,
                                XAuthorityControlAck {
                                    kind,
                                    transaction,
                                    surface,
                                    outcome: XAuthorityControlOutcome::AuthorityRejected,
                                },
                                completion,
                            )?;
                            continue;
                        }
                    };
                    core_event_selections
                        .lock()
                        .map_err(|_| {
                            X11SetupSocketError::new("X11 core event selection lock poisoned")
                        })?
                        .observe_unmapped(window);
                    if was_active {
                        vec![encode_x_client_event(
                            byte_order,
                            XClientEvent::UnmapNotify {
                                sequence: event_sequence,
                                event: window,
                                window,
                                from_configure: false,
                            },
                        )]
                    } else {
                        Vec::new()
                    }
                }
            };

            // The records are written before the acknowledgement, and this
            // return is one of the audited edges: if it fails the effect has
            // partly happened and no acknowledgement follows. The completion
            // record stays in its applying phase rather than being closed as
            // unexecuted.
            write_x11_control_records(&stream, &output_wire, byte_order, &sequence, records)?;
            channels.send_ack_for(
                client,
                XAuthorityControlAck {
                    kind,
                    transaction,
                    surface,
                    outcome: XAuthorityControlOutcome::Delivered,
                },
                completion,
            )?;
        }
        Ok(())
        };
        run()
    });
    Ok(X11ControlWriter { stop, thread })
}

#[cfg(unix)]
fn next_x11_metadata_generation(
    generations: &Mutex<BTreeMap<SurfaceId, u64>>,
    surface: SurfaceId,
) -> Result<u64, X11SetupSocketError> {
    let mut generations = generations
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 metadata generation lock poisoned"))?;
    let next = generations
        .get(&surface)
        .copied()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| X11SetupSocketError::new("X11 metadata generation exhausted"))?;
    generations.insert(surface, next);
    Ok(next)
}

#[cfg(unix)]
/// Wait for control runtime work to finish, or for the waiter to be told to
/// stop. Returns false when the wait was cancelled.
#[cfg(unix)]
fn wait_for_x11_control_runtime(
    control_runtime_pending: &AtomicUsize,
    stop: Option<&AtomicBool>,
) -> bool {
    while control_runtime_pending.load(Ordering::Acquire) != 0 {
        if stop.is_some_and(|stop| stop.load(Ordering::Acquire)) {
            return false;
        }
        std::thread::yield_now();
    }
    true
}

#[cfg(unix)]
fn lock_x11_request_runtime<'a>(
    runtime: &'a Mutex<XAuthorityRuntime>,
    control_runtime_pending: &AtomicUsize,
) -> Result<std::sync::MutexGuard<'a, XAuthorityRuntime>, X11SetupSocketError> {
    loop {
        wait_for_x11_control_runtime(control_runtime_pending, None);
        let runtime = runtime
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?;
        // A control can become pending between the pre-lock check and mutex
        // acquisition. Recheck while holding the lock so that request work
        // cannot overtake an already-waiting focus or configure command.
        if control_runtime_pending.load(Ordering::Acquire) == 0 {
            return Ok(runtime);
        }
        drop(runtime);
        std::thread::yield_now();
    }
}

#[cfg(unix)]
fn lock_x11_control_runtime<'a>(
    runtime: &'a Mutex<XAuthorityRuntime>,
    control_runtime_pending: &AtomicUsize,
) -> Result<std::sync::MutexGuard<'a, XAuthorityRuntime>, X11SetupSocketError> {
    control_runtime_pending.fetch_add(1, Ordering::AcqRel);
    let result = runtime.lock();
    control_runtime_pending.fetch_sub(1, Ordering::AcqRel);
    result.map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))
}

#[cfg(unix)]
fn write_control_u32(byte_order: XByteOrder, out: &mut [u8], value: u32) {
    let bytes = match byte_order {
        XByteOrder::LittleEndian => value.to_le_bytes(),
        XByteOrder::BigEndian => value.to_be_bytes(),
    };
    out.copy_from_slice(&bytes);
}

#[cfg(unix)]
fn clamp_engine_i16(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

#[cfg(unix)]
fn x11_selected_xi_event_window(
    authority: &crate::XInputAuthorityState,
    namespace: NamespaceId,
    owner: u64,
    ancestry: &[XResourceId],
    device: u16,
    event_type: u16,
) -> Option<XResourceId> {
    ancestry
        .iter()
        .find(|window| {
            authority.xi_event_selected(namespace, owner, **window, device, event_type)
        })
        .copied()
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
struct X11XiPointerDelivery {
    window: XResourceId,
    child: XResourceId,
    event_x: i16,
    event_y: i16,
    ancestry_depth: usize,
}

#[cfg(unix)]
fn x11_xi_pointer_delivery(
    selections: &XCoreEventSelectionState,
    surface_window: XResourceId,
    event_ancestry: &[XResourceId],
    selected_window: Option<XResourceId>,
    event_x: i16,
    event_y: i16,
) -> Option<X11XiPointerDelivery> {
    let window = selected_window?;
    let selected_index = event_ancestry
        .iter()
        .position(|candidate| *candidate == window)?;
    let child = selected_index
        .checked_sub(1)
        .and_then(|index| event_ancestry.get(index).copied())
        .unwrap_or(XResourceId::NONE);
    let (event_x, event_y) = selections.pointer_event_coordinates(
        surface_window,
        window,
        event_x,
        event_y,
    );
    Some(X11XiPointerDelivery {
        window,
        child,
        event_x,
        event_y,
        ancestry_depth: selected_index,
    })
}

#[cfg(unix)]
fn x11_pointer_surface_window(
    target_window: Option<XResourceId>,
    surface: SurfaceId,
    surface_windows: &Mutex<BTreeMap<SurfaceId, XResourceId>>,
) -> Result<Option<XResourceId>, X11SetupSocketError> {
    if target_window.is_some() {
        return Ok(target_window);
    }
    Ok(surface_windows
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 surface/window map lock poisoned"))?
        .get(&surface)
        .copied())
}

#[cfg(unix)]
include!("writers/records.rs");
include!("writers/blocked_send.rs");
include!("writers/ordered_delivery.rs");
include!("writers/ordered_serving.rs");
include!("writers/ordered_preparation.rs");
include!("writers/input.rs");

include!("writers/xi_source.rs");
