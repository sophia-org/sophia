/// The runtime and connection projection changed in one guarded interval.
/// This says nothing about FocusOut records, socket flush, or release debt.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct X11AppliedFocus {
    previous_authority: XResourceId,
    previous_revert_to: u8,
    previous_routed: XResourceId,
    window: XResourceId,
    revert_to: u8,
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum X11FocusApplyError {
    State(PrivateAppliedRegistryRefusal),
    Runtime(crate::XAuthorityRuntimeError),
    Superseded,
}

/// Both Engine and core callers name the exact runtime semantics. Engine
/// ClearFocus supplies root/revert=1 but deliberately publishes no key target.
/// Core focus=None supplies zero; an explicit core root remains a real target.
#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
enum X11FocusChange {
    Surface { window: XResourceId },
    Clear,
    Core { window: XResourceId, revert_to: u8 },
}

#[cfg(unix)]
impl X11FocusChange {
    fn window(self) -> XResourceId {
        match self {
            Self::Surface { window } | Self::Core { window, .. } => window,
            Self::Clear => XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
        }
    }
    fn revert_to(self) -> u8 {
        match self {
            Self::Core { revert_to, .. } => revert_to,
            Self::Surface { .. } | Self::Clear => 1,
        }
    }
    fn has_key_target(self) -> bool {
        !matches!(self, Self::Clear) && self.window().local.raw() != 0
    }
}

/// Actual focus producer for the control writer and the core dispatcher.
/// The caller already owns the outer runtime guard, with no X/selection guard.
/// Private common is therefore beneath outer runtime here. The private
/// executor/native cleanup must not acquire outer runtime while holding common.
/// The checked-in producer call sites are supplied in the integration patch;
/// this helper alone does not wire dispatch or establish writer completion.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
fn x11_apply_focus_change(
    runtime: &mut XAuthorityRuntime,
    namespace: NamespaceId,
    client: XServerFrontendClientId,
    focused_projection: &AtomicU64,
    routing: Option<&XServerFrontendRouteRegistry>,
    claim: Option<&PrivateFocusClaim>,
    change: X11FocusChange,
) -> Result<X11AppliedFocus, X11FocusApplyError> {
    if let Some(routing) = routing
        && routing.private_applied.get().is_some()
    {
        let claim = claim.ok_or(X11FocusApplyError::State(
            PrivateAppliedRegistryRefusal::MissingFocusClaim,
        ))?;
        return routing.apply_private_focus(
            runtime,
            namespace,
            client,
            focused_projection,
            claim,
            change,
        );
    }
    let (previous_authority, previous_revert_to) = runtime.input_focus(namespace);
    let previous_routed = XResourceId::new(focused_projection.load(Ordering::Acquire), 1);
    runtime
        .set_input_focus(namespace, change.window(), change.revert_to())
        .map_err(X11FocusApplyError::Runtime)?;
    focused_projection.store(change.window().local.raw(), Ordering::Release);
    Ok(X11AppliedFocus {
        previous_authority,
        previous_revert_to,
        previous_routed,
        window: change.window(),
        revert_to: change.revert_to(),
    })
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum X11DependentFocusEffect {
    ProjectionCleared,
    Superseded,
    Unproved,
}

/// Kept by the actual core request until its owned output stream has flushed.
/// The only completion operation writes and flushes its records; it accepts no
/// caller-supplied sent/published flag. Dropping it leaves focus unavailable.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Core source installation is in the call-site patch.
struct X11PendingFocusPublication {
    routing: XServerFrontendRouteRegistry,
    claim: PrivateFocusClaim,
    runtime: Arc<Mutex<XAuthorityRuntime>>,
    control_runtime_pending: Arc<AtomicUsize>,
    output: Arc<Mutex<UnixStream>>,
    output_control_pending: Arc<AtomicUsize>,
    output_wire: Arc<X11WirePermission>,
    revert_to: u8,
    records: Option<Vec<Vec<u8>>>,
    emission: X11CoreFocusEmission,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum X11CoreFocusEmission {
    Waiting,
    Written(usize),
    Indeterminate,
    Flushed,
    Superseded,
    Published,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
impl X11PendingFocusPublication {
    fn write_output(
        &mut self,
        event_sequence: &AtomicU16,
        sequence: u16,
    ) -> Result<(), X11SetupSocketError> {
        if matches!(self.emission, X11CoreFocusEmission::Indeterminate) {
            return Err(X11SetupSocketError::new(
                "private focus output is indeterminate",
            ));
        }
        if matches!(
            self.emission,
            X11CoreFocusEmission::Published | X11CoreFocusEmission::Superseded
        ) {
            return Ok(());
        }
        let records = self.records.as_ref().ok_or_else(|| {
            X11SetupSocketError::new("private focus source has not supplied its records")
        })?;
        if self.emission != X11CoreFocusEmission::Flushed {
            {
                // Bind completion to the source's retained transport, never a
                // socket supplied by a later caller. This is the core request's
                // actual output path, not a callback asserting another path sent.
                let mut stream =
                    lock_x11_non_control_output(
                        &self.output,
                        &self.output_wire,
                        &self.output_control_pending,
                        None,
                    )?
                        .expect("uncancellable source output");
                // Socket -> common is a bounded validation only. No reviewed
                // private path takes this socket while holding common. Outer
                // runtime is deliberately not acquired under the socket.
                if !self
                    .routing
                    .private_focus_output_current(&self.claim, self.revert_to)
                    .map_err(|cause| {
                        X11SetupSocketError::new(format!(
                            "private focus output unavailable: {cause:?}"
                        ))
                    })?
                {
                    self.emission = X11CoreFocusEmission::Superseded;
                    event_sequence.store(sequence, Ordering::Release);
                    return Ok(());
                }
                let first = match self.emission {
                    X11CoreFocusEmission::Written(next) => next,
                    _ => 0,
                };
                for (index, bytes) in records.iter().enumerate().skip(first) {
                    // The record stays on this request owner during partial
                    // writes and unwind. Unknown output is never replayable.
                    self.emission = X11CoreFocusEmission::Indeterminate;
                    std::io::Write::write_all(&mut *stream, bytes).map_err(|error| {
                        X11SetupSocketError::new(format!(
                            "failed to write private focus output: {error}"
                        ))
                    })?;
                    self.emission = X11CoreFocusEmission::Written(index + 1);
                }
                self.emission = X11CoreFocusEmission::Indeterminate;
                std::io::Write::flush(&mut *stream).map_err(|error| {
                    X11SetupSocketError::new(format!(
                        "failed to flush private focus output: {error}"
                    ))
                })?;
                self.emission = X11CoreFocusEmission::Flushed;
                event_sequence.store(sequence, Ordering::Release);
            }
        }
        // The socket is released before taking outer runtime. Recheck the
        // actual runtime while that guard excludes ordinary mutation, then
        // publish under common only if the exact source effect still agrees.
        let runtime = lock_x11_request_runtime(&self.runtime, &self.control_runtime_pending)?;
        let published = self
            .routing
            .publish_private_focus_after_output(&runtime, &self.claim, self.revert_to)
            .map_err(|cause| {
                X11SetupSocketError::new(format!(
                    "private focus publication unavailable: {cause:?}"
                ))
            })?;
        self.emission = if published {
            X11CoreFocusEmission::Published
        } else {
            X11CoreFocusEmission::Superseded
        };
        Ok(())
    }
}

/// An older notification remains owed, but it cannot clear a newer applied
/// focus claim. None in private mode is unproved, never ordinary permission.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
fn x11_apply_dependent_focus_out(
    namespace: NamespaceId,
    client: XServerFrontendClientId,
    window: XResourceId,
    focused_projection: &AtomicU64,
    routing: Option<&XServerFrontendRouteRegistry>,
    claim: Option<&PrivateFocusClaim>,
) -> Result<X11DependentFocusEffect, PrivateAppliedRegistryRefusal> {
    if let Some(routing) = routing
        && routing.private_applied.get().is_some()
    {
        let Some(claim) = claim else {
            return Ok(X11DependentFocusEffect::Unproved);
        };
        return routing.apply_private_focus_out(
            namespace,
            client,
            window,
            focused_projection,
            claim,
        );
    }
    focused_projection.store(u64::from(X_SETUP_DEFAULT_ROOT), Ordering::Release);
    Ok(X11DependentFocusEffect::ProjectionCleared)
}

#[cfg(unix)]
fn x11_focus_event_record(
    byte_order: XByteOrder,
    sequence: u16,
    window: XResourceId,
    focused: bool,
) -> Vec<u8> {
    encode_x_client_event(
        byte_order,
        XClientEvent::Focus {
            sequence,
            focused,
            detail: 3,
            event: window,
            mode: 0,
        },
    )
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
struct X11FocusEventState {
    time_msec: u32,
    root_x: i16,
    root_y: i16,
    event_x: i16,
    event_y: i16,
    state: u16,
}

#[cfg(unix)]
fn x11_focus_event_state(
    selections: &XCoreEventSelectionState,
    window: XResourceId,
    modifiers: u16,
    time_msec: u32,
) -> X11FocusEventState {
    let pointer = selections.query_pointer(window);
    X11FocusEventState {
        time_msec,
        root_x: pointer.map_or(0, |pointer| pointer.root_x),
        root_y: pointer.map_or(0, |pointer| pointer.root_y),
        event_x: pointer.map_or(0, |pointer| pointer.win_x),
        event_y: pointer.map_or(0, |pointer| pointer.win_y),
        state: pointer.map_or(modifiers & 0xff, |pointer| {
            (pointer.mask & !0xff) | (modifiers & 0xff)
        }),
    }
}

#[cfg(unix)]
fn encode_xi_focus_event(
    byte_order: XByteOrder,
    sequence: u16,
    window: XResourceId,
    focused: bool,
    state: X11FocusEventState,
) -> Vec<u8> {
    let mut out = vec![0; 76];
    out[0] = 35;
    out[1] = crate::X_INPUT_MAJOR_OPCODE;
    write_xi_u16(byte_order, &mut out[2..4], sequence);
    write_xi_u32(byte_order, &mut out[4..8], 11);
    write_xi_u16(byte_order, &mut out[8..10], if focused { 9 } else { 10 });
    write_xi_u16(byte_order, &mut out[10..12], 3);
    write_xi_u32(byte_order, &mut out[12..16], state.time_msec);
    write_xi_u16(byte_order, &mut out[16..18], 3);
    out[18] = 0;
    out[19] = 3;
    write_xi_u32(byte_order, &mut out[20..24], X_SETUP_DEFAULT_ROOT);
    write_xi_u32(
        byte_order,
        &mut out[24..28],
        u32::try_from(window.local.raw()).unwrap_or_default(),
    );
    write_xi_u32(
        byte_order,
        &mut out[32..36],
        (i32::from(state.root_x) << 16) as u32,
    );
    write_xi_u32(
        byte_order,
        &mut out[36..40],
        (i32::from(state.root_y) << 16) as u32,
    );
    write_xi_u32(
        byte_order,
        &mut out[40..44],
        (i32::from(state.event_x) << 16) as u32,
    );
    write_xi_u32(
        byte_order,
        &mut out[44..48],
        (i32::from(state.event_y) << 16) as u32,
    );
    out[48] = 1;
    out[49] = 1;
    write_xi_u16(byte_order, &mut out[50..52], 1);
    let modifiers = u32::from(state.state & 0xff);
    write_xi_u32(byte_order, &mut out[52..56], modifiers);
    write_xi_u32(byte_order, &mut out[64..68], modifiers);
    let buttons = (1_u8..=5).fold(0_u32, |buttons, button| {
        let core_mask = 1_u16 << (u32::from(button) + 7);
        if state.state & core_mask == 0 {
            buttons
        } else {
            buttons | (1_u32 << button)
        }
    });
    write_xi_u32(byte_order, &mut out[72..76], buttons);
    out
}

#[cfg(unix)]
struct X11FocusRecordContext<'a> {
    byte_order: XByteOrder,
    sequence: u16,
    namespace: NamespaceId,
    client: XServerFrontendClientId,
    selections: &'a XCoreEventSelectionState,
    input_authority: Option<&'a crate::XInputAuthorityState>,
    modifiers: u16,
    time_msec: u32,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
enum X11FocusRecordRequest {
    Event {
        window: XResourceId,
        focused: bool,
        time_msec: u32,
    },
    Surface {
        window: XResourceId,
        previous_authority: XResourceId,
        previous_routed: XResourceId,
        transition: Option<X11FocusTransition>,
    },
    Clear {
        root: XResourceId,
        previous_routed: XResourceId,
        transition: Option<X11FocusTransition>,
    },
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn x11_focus_records(
    byte_order: XByteOrder,
    sequence: u16,
    namespace: NamespaceId,
    client: XServerFrontendClientId,
    core_event_selections: &Mutex<XCoreEventSelectionState>,
    input_authority: Option<&Arc<Mutex<crate::XInputAuthorityState>>>,
    modifiers: u16,
    request: X11FocusRecordRequest,
) -> Result<Vec<Vec<u8>>, X11SetupSocketError> {
    let input_authority = input_authority
        .map(|authority| {
            authority
                .lock()
                .map_err(|_| X11SetupSocketError::new("X11 input authority lock poisoned"))
        })
        .transpose()?;
    // The input writer reads XI source selections under this same order.
    // Holding selections while waiting for input authority forms a cycle
    // with that writer, which can already own input authority and need these
    // selections. Keep the coherent snapshot, acquiring authority first.
    let selections = core_event_selections
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 core event selection lock poisoned"))?;
    let context = X11FocusRecordContext {
        byte_order,
        sequence,
        namespace,
        client,
        selections: &selections,
        input_authority: input_authority.as_deref(),
        modifiers,
        time_msec: x11_server_time_msec(),
    };
    match request {
        X11FocusRecordRequest::Event {
            window,
            focused,
            time_msec,
        } => Ok(x11_selected_focus_records(
            &X11FocusRecordContext {
                time_msec,
                ..context
            },
            window,
            focused,
        )),
        X11FocusRecordRequest::Surface {
            window,
            previous_authority,
            previous_routed,
            transition,
        } => x11_focus_surface_records(
            context,
            window,
            previous_authority,
            previous_routed,
            transition,
        ),
        X11FocusRecordRequest::Clear {
            root,
            previous_routed,
            transition,
        } => x11_clear_focus_records(context, root, previous_routed, transition),
    }
}

#[cfg(unix)]
fn x11_selected_focus_records(
    context: &X11FocusRecordContext<'_>,
    window: XResourceId,
    focused: bool,
) -> Vec<Vec<u8>> {
    let core_selected = context.selections.focus_selected(window);
    let event_type = if focused { 9 } else { 10 };
    let xi_selected = context.input_authority.is_some_and(|authority| {
        authority.xi_event_selected(
            context.namespace,
            context.client.raw(),
            window,
            3,
            event_type,
        )
    });
    tracing::info!(
        "sophia_x11_focus_delivery schema=1 client={} window={} focused={} core_selected={} xi2_selected={} content=redacted",
        context.client.raw(),
        window.local.raw(),
        focused,
        core_selected,
        xi_selected,
    );
    let mut records = Vec::with_capacity(2);
    if core_selected {
        records.push(x11_focus_event_record(
            context.byte_order,
            context.sequence,
            window,
            focused,
        ));
    }
    if xi_selected {
        records.push(encode_xi_focus_event(
            context.byte_order,
            context.sequence,
            window,
            focused,
            x11_focus_event_state(
                context.selections,
                window,
                context.modifiers,
                context.time_msec,
            ),
        ));
    }
    records
}

#[cfg(unix)]
fn x11_focus_surface_records(
    mut context: X11FocusRecordContext<'_>,
    window: XResourceId,
    previous_authority: XResourceId,
    previous_routed: XResourceId,
    transition: Option<X11FocusTransition>,
) -> Result<Vec<Vec<u8>>, X11SetupSocketError> {
    let transition = transition.unwrap_or_else(|| {
        if previous_authority == window && previous_routed == window {
            X11FocusTransition::Unchanged
        } else {
            X11FocusTransition::Enter {
                previous: (previous_routed != window
                    && previous_routed.local.raw() != u64::from(X_SETUP_DEFAULT_ROOT))
                .then_some(previous_routed),
                time_msec: x11_server_time_msec(),
            }
        }
    });
    match transition {
        X11FocusTransition::Unchanged => Ok(Vec::new()),
        X11FocusTransition::Enter {
            previous,
            time_msec,
        } => {
            context.time_msec = time_msec;
            let mut records = Vec::with_capacity(4);
            if let Some(previous) = previous {
                records.extend(x11_selected_focus_records(&context, previous, false));
            }
            records.extend(x11_selected_focus_records(&context, window, true));
            Ok(records)
        }
        X11FocusTransition::Clear { .. } => Err(X11SetupSocketError::new(
            "X11 routed focus transition mismatched FocusSurface",
        )),
    }
}

#[cfg(unix)]
fn x11_clear_focus_records(
    mut context: X11FocusRecordContext<'_>,
    root: XResourceId,
    previous_routed: XResourceId,
    transition: Option<X11FocusTransition>,
) -> Result<Vec<Vec<u8>>, X11SetupSocketError> {
    let (previous, time_msec) = match transition {
        Some(X11FocusTransition::Clear {
            previous,
            time_msec,
        }) => (previous, time_msec),
        None if previous_routed != root => (Some(previous_routed), x11_server_time_msec()),
        None => (None, x11_server_time_msec()),
        Some(_) => {
            return Err(X11SetupSocketError::new(
                "X11 routed focus transition mismatched ClearFocus",
            ));
        }
    };
    context.time_msec = time_msec;
    Ok(previous
        .map(|previous| x11_selected_focus_records(&context, previous, false))
        .unwrap_or_default())
}

#[cfg(unix)]
/// Write control records for this connection.
///
/// THROUGH THE SAME BOUNDARY as every other post-exposure writer, so a barred
/// wire stops control too: control is the writer with priority, not the writer
/// permitted to follow a half-finished event.
///
/// It does NOT wait on the pending-control counter. That counter is what other
/// writers yield to; making control wait on it would make control yield to
/// itself.
fn write_x11_control_records(
    stream: &Arc<Mutex<UnixStream>>,
    wire: &X11WirePermission,
    byte_order: XByteOrder,
    sequence: &AtomicU16,
    records: Vec<Vec<u8>>,
) -> Result<(), X11SetupSocketError> {
    let mut stream = enter_x11_wire(stream, wire)?;
    let event_sequence = sequence.load(Ordering::Acquire);
    for mut record in records {
        write_xi_u16(byte_order, &mut record[2..4], event_sequence);
        if std::env::var_os("SOPHIA_X11_AUTHORITY_TRACE").is_some() {
            tracing::trace!(
                "sophia_x11_socket_write schema=1 writer=control bytes={} payload_redacted=true",
                record.len(),
            );
        }
        stream
            .write_all(&record)
            .map_err(|error| x11_peer_write_error("failed to write X11 control event", error))?;
    }
    stream
        .flush()
        .map_err(|error| x11_peer_write_error("failed to flush X11 control event", error))
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)] // The core source retains its exact runtime and transport owners.
fn x11_dispatch_private_focus(
    runtime: &mut XAuthorityRuntime,
    context: crate::XDispatchContext,
    client: XServerFrontendClientId,
    projection: &AtomicU64,
    routing: &XServerFrontendRouteRegistry,
    window: XResourceId,
    revert_to: u8,
    time: crate::XTimestamp,
    runtime_owner: Arc<Mutex<XAuthorityRuntime>>,
    control_runtime_pending: Arc<AtomicUsize>,
    output: Arc<Mutex<UnixStream>>,
    output_control_pending: Arc<AtomicUsize>,
    output_wire: Arc<X11WirePermission>,
) -> Result<(XDispatchResult, Option<X11PendingFocusPublication>), X11SetupSocketError> {
    // Refusals and the clock are decided before anything is reserved. A
    // request the protocol discards must leave no claim behind and publish
    // no focus, or the ordering layer would wait on a change that never came.
    let standing = runtime.input_focus(context.namespace).0;
    if let Err(error) = runtime.validate_input_focus(context.namespace, window, revert_to) {
        return Ok((
            crate::dispatch::input_focus_dispatch_result(
                context,
                window,
                standing,
                crate::dispatch::XFocusRequestOutcome::Refused(error),
            ),
            None,
        ));
    }
    let Some(effective_time) =
        runtime.focus_time_admits(context.namespace, time, context.server_time)
    else {
        return Ok((
            crate::dispatch::input_focus_dispatch_result(
                context,
                window,
                standing,
                crate::dispatch::XFocusRequestOutcome::Ignored,
            ),
            None,
        ));
    };
    let claim = routing
        .reserve_private_focus(client, window)
        .map_err(|cause| {
            X11SetupSocketError::new(format!("private core focus claim unavailable: {cause:?}"))
        })?
        .ok_or_else(|| X11SetupSocketError::new("private core focus has no origin"))?;
    let mut previous = runtime.input_focus(context.namespace).0;
    let mut pending = None;
    let result = match x11_apply_focus_change(
        runtime,
        context.namespace,
        client,
        projection,
        Some(routing),
        Some(&claim),
        X11FocusChange::Core { window, revert_to },
    ) {
        Ok(applied) => {
            // The dependent producer really cleared this connection projection.
            // Same-window runtime reassertion still owes the restoring FocusIn.
            if previous == window && applied.previous_routed != window {
                previous = applied.previous_routed;
            }
            pending = Some(X11PendingFocusPublication {
                routing: routing.clone(),
                claim,
                runtime: runtime_owner,
                control_runtime_pending,
                output,
                output_control_pending,
                output_wire,
                revert_to,
                records: None,
                emission: X11CoreFocusEmission::Waiting,
            });
            runtime.note_focus_change(context.namespace, effective_time);
            crate::dispatch::XFocusRequestOutcome::Applied
        }
        Err(X11FocusApplyError::Runtime(cause)) => {
            crate::dispatch::XFocusRequestOutcome::Refused(cause)
        }
        Err(
            X11FocusApplyError::Superseded
            | X11FocusApplyError::State(
                PrivateAppliedRegistryRefusal::MissingAdmission
                | PrivateAppliedRegistryRefusal::AdmissionClosed
                | PrivateAppliedRegistryRefusal::ForeignOrigin,
            ),
        ) => {
            return Ok((
                XDispatchResult {
                    response: None,
                    outputs: vec![crate::XClientOutput::Error(crate::XClientError {
                        code: crate::XErrorCode::BadAccess,
                        sequence: context.sequence,
                        resource_id: u32::try_from(window.local.raw()).unwrap_or(0),
                        minor_code: 0,
                        major_code: context.major_opcode,
                    })],
                    metadata_candidates: Vec::new(),
                },
                None,
            ));
        }
        Err(cause) => {
            return Err(X11SetupSocketError::new(format!(
                "private core focus unavailable: {cause:?}"
            )));
        }
    };
    Ok((
        crate::dispatch::input_focus_dispatch_result(context, window, previous, result),
        pending,
    ))
}
