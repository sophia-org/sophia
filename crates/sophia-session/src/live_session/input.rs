#[path = "input/floating_pointer.rs"]
mod floating_pointer;
use floating_pointer::*;
#[path = "input/explicit_pointer_grab.rs"]
mod explicit_pointer_grab;
use explicit_pointer_grab::*;
#[path = "input/lease_routing.rs"]
mod lease_routing;
use lease_routing::*;

#[path = "input/pointer_focus.rs"]
mod pointer_focus;
use pointer_focus::*;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PhysicalInputRouteReport {
    /// What a full ingress queue cost this pass. Non-zero means the endpoint
    /// epoch must close, which is the barrier that replaces the records lost.
    ingress_saturation: RoutedInputIngressSaturation,
    events: usize,
    wm_actions: Vec<WmActionId>,
    policy_inputs: Vec<PhysicalPolicyInput>,
    launcher_events: Vec<sophia_engine::LauncherInputEvent>,
    reference_operations: Vec<(sophia_protocol::OutputId,u64,sophia_protocol::ShellReferenceOperation)>,
    chrome_activations: Vec<(sophia_protocol::OutputId, WmActionId)>,
    content_activations: Vec<sophia_engine::PresentedContentTarget>,
    descriptor_activations: Vec<(sophia_protocol::ToplevelActionCapabilityRef, u64)>,
    chrome_captures_started: usize,
    chrome_actions_activated: usize,
    chrome_captures_cancelled: usize,
    chrome_events_consumed: usize,
    wm_pointer_gestures: Vec<sophia_protocol::WmPointerGestureCompleted>,
    wm_pointer_interactions: Vec<FloatingPointerPolicyInteraction>,
    floating_outline: FloatingPointerOutlineUpdate,
    keys_observed: usize,
    keys_suppressed_no_focus: usize,
    keys_suppressed_stale_focus: usize,
    key_targets: Vec<SurfaceId>,
    routed_key_presses: Vec<(u64, u64)>,
    deferred_key_presses: Vec<(u64, u64)>,
    pointer_buttons_observed: usize,
    pointer_buttons_suppressed_no_target: usize,
    pointer_buttons_suppressed_by_policy: usize,
    pointer_buttons_routed: usize,
    pointer_lease_waits: usize,
    pointer_lease_rejections: usize,
    pointer_button_targets: Vec<SurfaceId>,
    pointer_focus_targets: Vec<SurfaceId>,
    pointer_axes_observed: usize,
    pointer_axes_routed: usize,
    pointer_axis_targets: Vec<SurfaceId>,
    keys_routed: usize,
    pointer_events: usize,
    pointer_routed: usize,
    deliveries: Vec<XAuthorityInputDeliveryId>,
    emergency_exit: bool,
    return_suppressed: bool,
    virtual_terminal: Option<u8>,
    virtual_terminal_trigger_keycode: Option<u32>,
    virtual_terminal_modifier_keycodes: [Option<u32>; 4],
    virtual_terminal_modifier_releases: usize,
    pointer_focus_handoff_expired: bool,
    pointer_focus_handoff_stale_drops: usize,
    pointer_focus_handoff_capacity_drops: usize,
    pointer_focus_handoff_released: Option<(SurfaceId, usize)>,
    keyboard_focus_handoff_expired: bool,
    keyboard_focus_handoff_stale_drops: usize,
    keyboard_focus_handoff_capacity_drops: usize,
    keyboard_focus_handoff_released: Option<(SurfaceId, usize)>,
    pointer_boundary_entries: Vec<(sophia_engine::PointerBoundaryContact, Option<usize>)>,
    pointer_boundary_reversals: Vec<(sophia_engine::PointerBoundaryContact, Option<usize>)>,
    pointer_output_transitions: Vec<(sophia_engine::PointerOutputTransition, bool)>,
}

type SessionPointerPlacement = sophia_engine::OutputUnionPointerState;

trait RoutedInputIngress {
    fn try_send(
        &self,
        route: XAuthorityRoutedInput,
    ) -> Result<(), std::sync::mpsc::TrySendError<XAuthorityRoutedInput>>;

    /// The queue's bound, so a saturation report names what was exhausted.
    fn capacity(&self) -> usize;
}

impl RoutedInputIngress for XAuthorityRoutedInputSender {
    fn try_send(
        &self,
        route: XAuthorityRoutedInput,
    ) -> Result<(), std::sync::mpsc::TrySendError<XAuthorityRoutedInput>> {
        XAuthorityRoutedInputSender::try_send(self, route)
    }

    fn capacity(&self) -> usize {
        XAuthorityRoutedInputSender::capacity(self)
    }
}

#[cfg(test)]
impl RoutedInputIngress for SyncSender<XAuthorityRoutedInput> {
    fn try_send(
        &self,
        route: XAuthorityRoutedInput,
    ) -> Result<(), std::sync::mpsc::TrySendError<XAuthorityRoutedInput>> {
        SyncSender::try_send(self, route)
    }

    fn capacity(&self) -> usize {
        // A plain channel has no capacity accessor. The value reaches only a
        // diagnostic field, never an admission decision.
        8
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ApplicationRouteLeaseUpdateReport {
    confirmed: usize,
    rejected: usize,
    released: usize,
    stale: usize,
}

fn drain_application_route_lease_updates(
    receiver: &Receiver<XAuthorityRouteLeaseUpdate>,
    state: &mut ApplicationRouteLeaseState,
) -> ApplicationRouteLeaseUpdateReport {
    let mut report = ApplicationRouteLeaseUpdateReport::default();
    while let Ok(update) = receiver.try_recv() {
        let admission = update.admission.client_id;
        let authority_session_epoch = update.admission.auth_provenance.session_generation;
        let result = match update.kind {
            XAuthorityRouteLeaseUpdateKind::Confirmed => state.confirm(
                update.identity,
                update.target_surface,
                admission,
                authority_session_epoch,
            ),
            XAuthorityRouteLeaseUpdateKind::Rejected => state.reject(update.identity),
            XAuthorityRouteLeaseUpdateKind::Released => {
                state.frontend_release(update.identity, admission)
            }
        };
        match (update.kind, result) {
            (XAuthorityRouteLeaseUpdateKind::Confirmed, Ok(_)) => {
                report.confirmed = report.confirmed.saturating_add(1)
            }
            (XAuthorityRouteLeaseUpdateKind::Rejected, Ok(_)) => {
                report.rejected = report.rejected.saturating_add(1)
            }
            (XAuthorityRouteLeaseUpdateKind::Released, Ok(_)) => {
                report.released = report.released.saturating_add(1)
            }
            (_, Err(_)) => report.stale = report.stale.saturating_add(1),
        }
    }
    report
}

fn place_pointer_event_for_routing(
    event: &mut sophia_protocol::InputEventPacket,
    focused_surface: Option<SurfaceId>,
    input_layers: &[LayerSnapshot],
    pointer: &mut SessionPointerPlacement,
    buttons_only: bool,
) -> (bool, Option<sophia_engine::OutputUnionPointerPlacement>) {
    let placement = if let Some(raw) = event.global_position {
        let geometry = focused_surface.and_then(|surface| {
            input_layers
                .iter()
                .find(|layer| layer.surface == surface)
                .map(|layer| layer.geometry)
        });
        let placement = pointer.place(raw, geometry);
        event.global_position = Some(placement.position);
        Some(placement)
    } else {
        None
    };
    (
        !(buttons_only && matches!(event.kind, sophia_protocol::InputEventKind::PointerMotion)),
        placement,
    )
}

/// What the pointer path needs from a presented projection: the layers, the
/// indicator and chrome hit targets with their rectangles, and the output and
/// epoch they belong to.
type PointerInputProjection<'a> = (
    &'a [LayerSnapshot],
    &'a [sophia_engine::IndicatorChromeHitTarget],
    Option<sophia_protocol::Rect>,
    &'a [sophia_engine::PresentedChromeTarget],
    Option<sophia_protocol::Rect>,
    Option<&'a sophia_engine::PresentedContentBinding>,
    Option<sophia_protocol::OutputId>,
    u64,
);

fn input_projection_for_pointer<'a>(
    projections: Option<&'a [sophia_backend_live::LivePresentedInputProjection]>,
    pointer_outputs: Option<&[sophia_engine::HeadlessOutput]>,
    output_index: Option<usize>,
    fallback_layers: &'a [LayerSnapshot],
    fallback_output: Option<sophia_protocol::OutputId>,
    fallback_epoch: u64,
) -> PointerInputProjection<'a> {
    // A known output with no retired projection has no input authority yet.
    // Falling back to the primary head would turn empty-monitor motion into
    // an observation about an unrelated output.
    if let (Some(projections), Some(output)) = (projections,
        output_index.and_then(|i| pointer_outputs.and_then(|outputs| outputs.get(i))))
        && !projections.iter().any(|p| p.output == output.id) {
        return (&[], &[], None, &[], None, None, Some(output.id), 0);
    }
    output_index
        .and_then(|index| pointer_outputs.and_then(|outputs| outputs.get(index)))
        .and_then(|output| {
            projections.and_then(|projections| {
                projections
                    .iter()
                    .find(|projection| projection.output == output.id)
            })
        })
        .map_or(
            (
                fallback_layers,
                &[],
                None,
                &[],
                None,
                None,
                fallback_output,
                fallback_epoch,
            ),
            |projection| {
                (
                    projection.layers.as_slice(),
                    projection.chrome_targets.as_slice(),
                    projection.chrome_occlusion,
                    projection.descriptor_targets.as_slice(),
                    projection.descriptor_occlusion,
                    projection.content.as_ref(),
                    Some(projection.output),
                    projection.epoch,
                )
            },
        )
}

fn application_route_lease_for_request(
    request: &sophia_protocol::RoutedInputRequest,
    client_routes: &XAuthorityClientSurfaceRoutes,
    state: &mut ApplicationRouteLeaseState,
    input_output: Option<sophia_protocol::OutputId>,
    input_presentation_epoch: u64,
) -> Result<Option<sophia_protocol::ApplicationRouteLeaseIdentity>, Box<dyn std::error::Error>> {
    if let Some(lease) = state.lease(request.seat) {
        let is_initiating_boundary = matches!(
            request.kind,
            sophia_protocol::InputEventKind::PointerButton { button, .. }
                if lease.initiating_button == Some(button)
                    && lease.initiating_device == Some(request.device)
        );
        return Ok(is_initiating_boundary.then_some(lease.identity));
    }
    let sophia_protocol::InputEventKind::PointerButton {
        button,
        pressed: true,
    } = request.kind
    else {
        return Ok(None);
    };
    let Some(admission) = client_routes.admission_for_surface(request.target_surface) else {
        return Ok(None);
    };
    let Some(output) = input_output else {
        return Ok(None);
    };
    if input_presentation_epoch == 0 {
        return Ok(None);
    }
    let lease = state
        .begin_provisional(ApplicationRouteLeaseCandidate {
            seat: request.seat,
            origin: sophia_engine::ApplicationRouteLeaseOrigin::PointerBoundary,
            target_surface: request.target_surface,
            admission: admission.client_id,
            scope: ApplicationRouteScope {
                profile: admission.namespace.profile,
                authority: admission.namespace.id,
            },
            authority_session_epoch: admission.auth_provenance.session_generation,
            binding: sophia_engine::ApplicationRouteLeaseBinding::Bound { output, revision: input_presentation_epoch },
            initiating_device: Some(request.device),
            initiating_button: Some(button),
        })
        .map_err(|error| format!("failed to begin application route lease: {error:?}"))?;
    Ok(Some(lease.identity))
}

fn request_application_route_lease_release(
    state: &mut ApplicationRouteLeaseState,
    client_routes: &XAuthorityClientSurfaceRoutes,
    sender: &SyncSender<XAuthorityRouteLeaseRelease>,
    seat: sophia_protocol::SeatId,
    now_msec: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let lease = state
        .request_release(seat, now_msec)
        .map_err(|error| format!("failed to request application lease release: {error:?}"))?;
    let admission = client_routes
        .admission_for_surface(lease.target_surface)
        .filter(|admission| {
            admission.client_id == lease.admission
                && admission.auth_provenance.session_generation == lease.authority_session_epoch
        });
    let Some(admission) = admission else {
        // The authority already retired this route. Its exact old ownership
        // cannot be addressed through a replacement client or surface.
        let _ = state.acknowledge_release(lease.identity, lease.admission);
        return Ok(());
    };
    match sender.try_send(XAuthorityRouteLeaseRelease { identity: lease.identity, admission }) {
        Ok(()) => {}
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            // Releasing is already non-routable. The existing bounded release
            // deadline quarantines this admission if the notice cannot fit.
            crate::session_println!("sophia_live_input_lease schema=1 status=release_deferred reason=capacity");
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            let _ = state.acknowledge_release(lease.identity, lease.admission);
        }
    }
    Ok(())
}

fn advance_application_input_security_epoch(
    state: &mut ApplicationRouteLeaseState,
    input_sender: &XAuthorityRoutedInputSender,
    client_routes: &XAuthorityClientSurfaceRoutes,
    release_sender: &SyncSender<XAuthorityRouteLeaseRelease>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let revoked = state
        .security_transition()
        .map_err(|error| format!("failed to advance application input epoch: {error:?}"))?;
    if !input_sender.advance_control_epoch(state.control_epoch()) {
        return Err("X frontend rejected application input epoch advance".into());
    }
    for lease in &revoked {
        let Some(admission) = client_routes
            .admission_for_surface(lease.target_surface)
            .filter(|admission| {
                admission.client_id == lease.admission
                    && admission.auth_provenance.session_generation
                        == lease.authority_session_epoch
            })
        else {
            continue;
        };
        // Epoch application in the broker clears all active grabs and frozen
        // input. This exact release is a best-effort lifecycle acknowledgement,
        // not the security barrier itself.
        let _ = release_sender.try_send(XAuthorityRouteLeaseRelease {
            identity: lease.identity,
            admission,
        });
    }
    Ok(revoked.len())
}

struct PhysicalInputRoutingContext<'a> {
    focus: &'a InputFocusState,
    committed_surfaces: &'a [CommittedSurfaceState],
    input_layers: &'a [LayerSnapshot],
    input_projections: &'a [sophia_backend_live::LivePresentedInputProjection],
    pointer_outputs: &'a [sophia_engine::HeadlessOutput],
    surface_roles: &'a BTreeMap<SurfaceId, sophia_protocol::SurfacePresentationRole>,
    client_routes: &'a XAuthorityClientSurfaceRoutes,
    shortcuts: Option<&'a mut WmShortcutRouter>,
    input_sender: &'a XAuthorityRoutedInputSender,
    modifiers: &'a mut XCoreKeyboardMapper,
    key_repeat: &'a mut KeyRepeatState,
    key_repeat_map: &'a XkbKeymapSnapshot,
    client_keys: &'a mut SessionClientKeyState,
    emergency_chord: &'a mut EmergencyChordState,
    virtual_terminal_chord: &'a mut VirtualTerminalChordState,
    keyboard_coverage: &'a mut PhysicalKeyboardCoverage,
    pointer: &'a mut SessionPointerPlacement,
    pointer_routing_enabled: bool,
    pointer_proof_required: bool,
    pointer_buttons_only: bool,
    routing_mode: PhysicalInputRoutingMode,
    next_input_delivery: &'a mut u64,
    now_msec: u64,
    physical_text_proof: Option<&'a mut PhysicalTextProof>,
    keyboard_focus_handoff: &'a mut KeyboardFocusHandoffState,
    pointer_focus_handoff: &'a mut PointerFocusHandoffState,
    applied_client_focus: Option<SurfaceId>,
    floating_gesture: &'a mut FloatingPointerGestureState,
    application_route_leases: &'a mut ApplicationRouteLeaseState,
    pending_lease_input: &'a mut PendingLeaseInput,
    chrome_captures: &'a mut sophia_engine::ChromeCaptureState,
    descriptor_captures: &'a mut sophia_engine::PresentedChromeCaptureState,
    content_captures: &'a mut sophia_engine::ContentCaptureState,
    reference_capture: &'a mut sophia_engine::ReferenceSheetCapture,
    launcher_capture: &'a mut sophia_engine::LauncherCapture,
    launcher_keyboard: &'a mut sophia_engine::LauncherKeyboard,
    route_lease_release_sender: &'a SyncSender<XAuthorityRouteLeaseRelease>,
    input_output: Option<sophia_protocol::OutputId>,
    input_presentation_epoch: u64,
}

fn route_physical_input<P: NonBlockingInputPoller>(
    poller: &mut P,
    context: PhysicalInputRoutingContext<'_>,
) -> Result<PhysicalInputRouteReport, Box<dyn std::error::Error>> {
    let events = poller.poll_ready()?;
    let PhysicalInputRoutingContext {
        focus,
        committed_surfaces,
        input_layers,
        input_projections,
        pointer_outputs,
        surface_roles,
        client_routes,
        shortcuts,
        input_sender,
        modifiers,
        key_repeat,
        key_repeat_map,
        client_keys,
        emergency_chord,
        virtual_terminal_chord,
        keyboard_coverage,
        pointer,
        pointer_routing_enabled,
        pointer_proof_required,
        pointer_buttons_only,
        routing_mode,
        next_input_delivery,
        now_msec,
        physical_text_proof,
        keyboard_focus_handoff,
        pointer_focus_handoff,
        applied_client_focus,
        floating_gesture,
        application_route_leases,
        pending_lease_input,
        chrome_captures,
        descriptor_captures,
        content_captures,
        reference_capture,
        launcher_capture,
        launcher_keyboard,
        route_lease_release_sender,
        input_output,
        input_presentation_epoch,
    } = context;
    route_input_events_with_launcher(
        events,
        focus,
        committed_surfaces,
        input_layers,
        surface_roles,
        client_routes,
        input_sender,
        modifiers,
        key_repeat,
        key_repeat_map,
        client_keys,
        emergency_chord,
        virtual_terminal_chord,
        keyboard_coverage,
        shortcuts,
        pointer,
        pointer_routing_enabled,
        pointer_proof_required,
        pointer_buttons_only,
        routing_mode,
        next_input_delivery,
        now_msec,
        physical_text_proof,
        Some(keyboard_focus_handoff),
        Some(pointer_focus_handoff),
        applied_client_focus,
        Some(floating_gesture),
        Some(application_route_leases),
        Some(chrome_captures),
        Some(descriptor_captures),
        Some(content_captures),
        Some(route_lease_release_sender),
        input_output,
        input_presentation_epoch,
        Some(input_projections),
        Some(pointer_outputs),
        Some(reference_capture),
        Some((launcher_capture, launcher_keyboard)),
        Some(pending_lease_input),
    )
}

#[allow(clippy::too_many_arguments)]
fn route_input_events(
    events: Vec<sophia_protocol::InputEventPacket>,
    focus: &InputFocusState,
    committed_surfaces: &[CommittedSurfaceState],
    input_layers: &[LayerSnapshot],
    client_routes: &XAuthorityClientSurfaceRoutes,
    input_sender: &impl RoutedInputIngress,
    modifiers: &mut XCoreKeyboardMapper,
    key_repeat: &mut KeyRepeatState,
    key_repeat_map: &XkbKeymapSnapshot,
    client_keys: &mut SessionClientKeyState,
    emergency_chord: &mut EmergencyChordState,
    virtual_terminal_chord: &mut VirtualTerminalChordState,
    keyboard_coverage: &mut PhysicalKeyboardCoverage,
    shortcuts: Option<&mut WmShortcutRouter>,
    pointer: &mut SessionPointerPlacement,
    pointer_routing_enabled: bool,
    pointer_proof_required: bool,
    pointer_buttons_only: bool,
    routing_mode: PhysicalInputRoutingMode,
    next_input_delivery: &mut u64,
    now_msec: u64,
    physical_text_proof: Option<&mut PhysicalTextProof>,
    keyboard_focus_handoff: Option<&mut KeyboardFocusHandoffState>,
    applied_client_focus: Option<SurfaceId>,
) -> Result<PhysicalInputRouteReport, Box<dyn std::error::Error>> {
    let surface_roles = BTreeMap::new();
    route_input_events_with_pointer_focus(
        events,
        focus,
        committed_surfaces,
        input_layers,
        &surface_roles,
        client_routes,
        input_sender,
        modifiers,
        key_repeat,
        key_repeat_map,
        client_keys,
        emergency_chord,
        virtual_terminal_chord,
        keyboard_coverage,
        shortcuts,
        pointer,
        pointer_routing_enabled,
        pointer_proof_required,
        pointer_buttons_only,
        routing_mode,
        next_input_delivery,
        now_msec,
        physical_text_proof,
        keyboard_focus_handoff,
        None,
        applied_client_focus,
        None,
        None,
        None,
        None,
        None,
        None,
        0,
        None,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn route_input_events_with_pointer_focus(
    events: Vec<sophia_protocol::InputEventPacket>,
    focus: &InputFocusState,
    committed_surfaces: &[CommittedSurfaceState],
    input_layers: &[LayerSnapshot],
    surface_roles: &BTreeMap<SurfaceId, sophia_protocol::SurfacePresentationRole>,
    client_routes: &XAuthorityClientSurfaceRoutes,
    input_sender: &impl RoutedInputIngress,
    modifiers: &mut XCoreKeyboardMapper,
    key_repeat: &mut KeyRepeatState,
    key_repeat_map: &XkbKeymapSnapshot,
    client_keys: &mut SessionClientKeyState,
    emergency_chord: &mut EmergencyChordState,
    virtual_terminal_chord: &mut VirtualTerminalChordState,
    keyboard_coverage: &mut PhysicalKeyboardCoverage,
    shortcuts: Option<&mut WmShortcutRouter>,
    pointer: &mut SessionPointerPlacement,
    pointer_routing_enabled: bool,
    pointer_proof_required: bool,
    pointer_buttons_only: bool,
    routing_mode: PhysicalInputRoutingMode,
    next_input_delivery: &mut u64,
    now_msec: u64,
    physical_text_proof: Option<&mut PhysicalTextProof>,
    keyboard_focus_handoff: Option<&mut KeyboardFocusHandoffState>,
    pointer_focus_handoff: Option<&mut PointerFocusHandoffState>,
    applied_client_focus: Option<SurfaceId>,
    floating_gesture: Option<&mut FloatingPointerGestureState>,
    application_route_leases: Option<&mut ApplicationRouteLeaseState>,
    chrome_captures: Option<&mut sophia_engine::ChromeCaptureState>,
    descriptor_captures: Option<&mut sophia_engine::PresentedChromeCaptureState>,
    route_lease_release_sender: Option<&SyncSender<XAuthorityRouteLeaseRelease>>,
    input_output: Option<sophia_protocol::OutputId>,
    input_presentation_epoch: u64,
    input_projections: Option<&[sophia_backend_live::LivePresentedInputProjection]>,
    pointer_outputs: Option<&[sophia_engine::HeadlessOutput]>,
    reference_capture: Option<&mut sophia_engine::ReferenceSheetCapture>,
) -> Result<PhysicalInputRouteReport, Box<dyn std::error::Error>> {
    route_input_events_with_launcher(
        events,
        focus,
        committed_surfaces,
        input_layers,
        surface_roles,
        client_routes,
        input_sender,
        modifiers,
        key_repeat,
        key_repeat_map,
        client_keys,
        emergency_chord,
        virtual_terminal_chord,
        keyboard_coverage,
        shortcuts,
        pointer,
        pointer_routing_enabled,
        pointer_proof_required,
        pointer_buttons_only,
        routing_mode,
        next_input_delivery,
        now_msec,
        physical_text_proof,
        keyboard_focus_handoff,
        pointer_focus_handoff,
        applied_client_focus,
        floating_gesture,
        application_route_leases,
        chrome_captures,
        descriptor_captures,
        None,
        route_lease_release_sender,
        input_output,
        input_presentation_epoch,
        input_projections,
        pointer_outputs,
        reference_capture,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn route_input_events_with_launcher(
    events: Vec<sophia_protocol::InputEventPacket>,
    focus: &InputFocusState,
    committed_surfaces: &[CommittedSurfaceState],
    input_layers: &[LayerSnapshot],
    surface_roles: &BTreeMap<SurfaceId, sophia_protocol::SurfacePresentationRole>,
    client_routes: &XAuthorityClientSurfaceRoutes,
    input_sender: &impl RoutedInputIngress,
    modifiers: &mut XCoreKeyboardMapper,
    key_repeat: &mut KeyRepeatState,
    key_repeat_map: &XkbKeymapSnapshot,
    client_keys: &mut SessionClientKeyState,
    emergency_chord: &mut EmergencyChordState,
    virtual_terminal_chord: &mut VirtualTerminalChordState,
    keyboard_coverage: &mut PhysicalKeyboardCoverage,
    mut shortcuts: Option<&mut WmShortcutRouter>,
    pointer: &mut SessionPointerPlacement,
    pointer_routing_enabled: bool,
    pointer_proof_required: bool,
    pointer_buttons_only: bool,
    routing_mode: PhysicalInputRoutingMode,
    next_input_delivery: &mut u64,
    now_msec: u64,
    mut physical_text_proof: Option<&mut PhysicalTextProof>,
    mut keyboard_focus_handoff: Option<&mut KeyboardFocusHandoffState>,
    mut pointer_focus_handoff: Option<&mut PointerFocusHandoffState>,
    applied_client_focus: Option<SurfaceId>,
    mut floating_gesture: Option<&mut FloatingPointerGestureState>,
    mut application_route_leases: Option<&mut ApplicationRouteLeaseState>,
    mut chrome_captures: Option<&mut sophia_engine::ChromeCaptureState>,
    mut descriptor_captures: Option<&mut sophia_engine::PresentedChromeCaptureState>,
    mut content_captures: Option<&mut sophia_engine::ContentCaptureState>,
    route_lease_release_sender: Option<&SyncSender<XAuthorityRouteLeaseRelease>>,
    input_output: Option<sophia_protocol::OutputId>,
    input_presentation_epoch: u64,
    input_projections: Option<&[sophia_backend_live::LivePresentedInputProjection]>,
    pointer_outputs: Option<&[sophia_engine::HeadlessOutput]>,
    mut reference_capture: Option<&mut sophia_engine::ReferenceSheetCapture>,
    mut launcher: Option<(&mut sophia_engine::LauncherCapture, &mut sophia_engine::LauncherKeyboard)>,
    mut pending_lease_input: Option<&mut PendingLeaseInput>,
) -> Result<PhysicalInputRouteReport, Box<dyn std::error::Error>> {
    let mut report = PhysicalInputRouteReport {
        ingress_saturation: RoutedInputIngressSaturation::default(),
        events: events.len(),
        wm_actions: Vec::new(),
        policy_inputs: Vec::new(),
        reference_operations: Vec::new(),
        launcher_events: Vec::new(),
        chrome_activations: Vec::new(),
        descriptor_activations: Vec::new(),
        content_activations: Vec::new(),
        chrome_captures_started: 0,
        chrome_actions_activated: 0,
        chrome_captures_cancelled: 0,
        chrome_events_consumed: 0,
        wm_pointer_gestures: Vec::new(),
        wm_pointer_interactions: Vec::new(),
        floating_outline: FloatingPointerOutlineUpdate::Unchanged,
        keys_observed: 0,
        keys_suppressed_no_focus: 0,
        keys_suppressed_stale_focus: 0,
        keys_routed: 0,
        key_targets: Vec::new(),
        routed_key_presses: Vec::new(),
        deferred_key_presses: Vec::new(),
        pointer_events: 0,
        pointer_buttons_observed: 0,
        pointer_buttons_suppressed_no_target: 0,
        pointer_buttons_suppressed_by_policy: 0,
        pointer_axes_observed: 0,
        pointer_routed: 0,
        pointer_buttons_routed: 0,
        pointer_lease_waits: 0,
        pointer_lease_rejections: 0,
        pointer_button_targets: Vec::new(),
        pointer_focus_targets: Vec::new(),
        pointer_axes_routed: 0,
        pointer_axis_targets: Vec::new(),
        deliveries: Vec::new(),
        emergency_exit: false,
        return_suppressed: false,
        virtual_terminal: None,
        virtual_terminal_trigger_keycode: None,
        virtual_terminal_modifier_keycodes: [None; 4],
        virtual_terminal_modifier_releases: 0,
        pointer_focus_handoff_expired: false,
        pointer_focus_handoff_stale_drops: 0,
        pointer_focus_handoff_capacity_drops: 0,
        pointer_focus_handoff_released: None,
        keyboard_focus_handoff_expired: false,
        keyboard_focus_handoff_stale_drops: 0,
        keyboard_focus_handoff_capacity_drops: 0,
        keyboard_focus_handoff_released: None,
        pointer_boundary_entries: Vec::new(),
        pointer_boundary_reversals: Vec::new(),
        pointer_output_transitions: Vec::new(),
    };
    if routing_mode == PhysicalInputRoutingMode::Full
        && let (Some(held), Some(state), Some(projections), Some(release_sender)) = (
            pending_lease_input.as_deref_mut(), application_route_leases.as_deref_mut(), input_projections, route_lease_release_sender,
        )
    {
        flush_held_lease_input(held, state, client_routes, projections, input_sender, release_sender,
            next_input_delivery, now_msec, &mut report)?;
    }
    let mut routed_events = VecDeque::new();
    if let Some(handoff) = keyboard_focus_handoff.as_deref_mut() {
        if handoff.cancel_if_target_stale(|target| {
            committed_surfaces
                .iter()
                .any(|committed| committed.surface == target)
                && client_routes.client_for_surface(target).is_some()
        }) {
            report.keyboard_focus_handoff_stale_drops = 1;
        } else {
            report.keyboard_focus_handoff_expired = handoff.expire(now_msec);
        }
        if routing_mode == PhysicalInputRoutingMode::Full
            && let Some(mut ready) = handoff.take_ready(applied_client_focus)
        {
            let released_target = applied_client_focus;
            let released_count = ready.len();
            routed_events.extend(ready.drain(..).map(|event| (event, true)));
            report.keyboard_focus_handoff_released =
                released_target.map(|surface| (surface, released_count));
        }
    }
    routed_events.extend(events.into_iter().map(|event| (event, false)));
    if let Some(handoff) = pointer_focus_handoff.as_deref_mut() {
        if handoff.cancel_if_target_stale(|target| {
            let present = sophia_engine::scene_contains_input_surface(input_layers, target)
                || input_projections.is_some_and(|projections| {
                    projections.iter().any(|projection| {
                        sophia_engine::scene_contains_input_surface(&projection.layers, target)
                    })
                });
            present
                && client_routes.client_for_surface(target).is_some()
        }) {
            report.pointer_focus_handoff_stale_drops = 1;
        } else {
            report.pointer_focus_handoff_expired = handoff.expire(now_msec);
        }
        if let Some(mut ready) = handoff.take_ready(applied_client_focus) {
            let released_target = applied_client_focus;
            let released_count = ready.len();
            while let Some(request) = ready.pop_front() {
                let is_button = matches!(
                    request.kind,
                    sophia_protocol::InputEventKind::PointerButton { .. }
                );
                let is_axis = matches!(
                    request.kind,
                    sophia_protocol::InputEventKind::PointerAxis { .. }
                );
                let target = request.target_surface;
                let delivery = XAuthorityInputDeliveryId::from_raw(*next_input_delivery);
                *next_input_delivery = next_input_delivery
                    .checked_add(1)
                    .ok_or("live-session input delivery ID exhausted")?;
                let route_lease = match application_route_leases.as_deref_mut() {
                    Some(state) => application_route_lease_for_request(
                        &request,
                        client_routes,
                        state,
                        input_output,
                        input_presentation_epoch,
                    )?,
                    None => None,
                };
                if !route_bounded_input(
                    input_sender,
                    XAuthorityRoutedInput {
                        request,
                        route_lease,
                        delivery: Some(delivery),
                        mode: XAuthorityRoutedInputMode::Deliver,
                    },
                    sophia_protocol::CapacityClass::Ordered,
                    &mut report.ingress_saturation,
                )? {
                    continue;
                }
                report.pointer_routed = report.pointer_routed.saturating_add(1);
                if is_button {
                    report.pointer_buttons_routed =
                        report.pointer_buttons_routed.saturating_add(1);
                    report.pointer_button_targets.push(target);
                }
                if is_axis {
                    report.pointer_axes_routed = report.pointer_axes_routed.saturating_add(1);
                    report.pointer_axis_targets.push(target);
                }
                report.deliveries.push(delivery);
            }
            report.pointer_focus_handoff_released =
                released_target.map(|surface| (surface, released_count));
        }
    }
    for (mut event, control_plane_applied) in routed_events {
        match event.kind {
            sophia_protocol::InputEventKind::Key { keycode, pressed } => {
                if !control_plane_applied {
                    report.keys_observed = report.keys_observed.saturating_add(1);
                    keyboard_coverage.observe_key(keycode, pressed);
                    let launcher_text=launcher.as_mut().map(|(capture,keyboard)|keyboard.observe(keycode,pressed,capture.active()));

                    match virtual_terminal_chord.observe_at_device(
                        event.device,
                        keycode,
                        pressed,
                        event.time_msec,
                    ) {
                    VirtualTerminalChordAction::Pass => {}
                    VirtualTerminalChordAction::Consume => continue,
                    VirtualTerminalChordAction::Activate(terminal) => {
                        report.virtual_terminal_trigger_keycode = Some(keycode);
                        report.virtual_terminal_modifier_keycodes =
                            virtual_terminal_chord
                                .pressed_modifier_keycodes_for(event.device);
                        keyboard_coverage.observe_virtual_terminal(terminal);
                        for modifier_keycode in virtual_terminal_chord
                            .pressed_modifier_keycodes_for(event.device)
                            .into_iter()
                            .flatten()
                        {
                            if let Some(shortcuts) = shortcuts.as_deref_mut() {
                                let _ = shortcuts.route_key(event.seat, modifier_keycode, false);
                            }
                            let _ = modifiers.map_evdev_key(modifier_keycode, false);
                            if let Some((_, keyboard)) = launcher.as_mut() {
                                // Releases may occur on the destination VT;
                                // release every local keyboard view before leaving.
                                let _ = keyboard.observe(modifier_keycode, false, false);
                            }
                            if routing_mode != PhysicalInputRoutingMode::Full {
                                continue;
                            }
                            let mut release = event.clone();
                            release.kind = sophia_protocol::InputEventKind::Key {
                                keycode: modifier_keycode,
                                pressed: false,
                            };
                            let release = match focus
                                .route_keyboard_event(release, committed_surfaces)
                            {
                                FocusedInputRoute::Routed(release) => release,
                                FocusedInputRoute::NoFocus(_)
                                | FocusedInputRoute::StaleFocus(_)
                                | FocusedInputRoute::UnsupportedEvent(_) => continue,
                            };
                            let Some(target_surface) = release.target_surface else {
                                continue;
                            };
                            let delivery =
                                XAuthorityInputDeliveryId::from_raw(*next_input_delivery);
                            *next_input_delivery =
                                next_input_delivery.checked_add(1).ok_or(
                                    "live-session input delivery ID exhausted",
                                )?;
                            // A release the client never sees is a modifier
                            // held down forever, so leave the key recorded as
                            // pressed and let the epoch close be the barrier.
                            if !route_bounded_input(
                                input_sender,
                                XAuthorityRoutedInput {
                                    request: sophia_protocol::RoutedInputRequest {
                                        serial: release.serial,
                                        seat: release.seat,
                                        device: release.device,
                                        time_msec: release.time_msec,
                                        target_surface,
                                        global_position: Point::default(),
                                        local_position: Point::default(),
                                        kind: release.kind,
                                    },
                                    route_lease: None,
                                    delivery: Some(delivery),
                                    mode: XAuthorityRoutedInputMode::Deliver,
                                },
                                sophia_protocol::CapacityClass::TerminatingBoundary,
                                &mut report.ingress_saturation,
                            )? {
                                continue;
                            }
                            client_keys.record_routed(
                                SessionClientPressedKey {
                                    surface: target_surface,
                                    seat: release.seat,
                                    device: release.device,
                                    keycode: modifier_keycode,
                                },
                                false,
                            );
                            report.keys_routed = report.keys_routed.saturating_add(1);
                            report.key_targets.push(target_surface);
                            report.virtual_terminal_modifier_releases = report
                                .virtual_terminal_modifier_releases
                                .saturating_add(1);
                            report.deliveries.push(delivery);
                        }
                        report.virtual_terminal = Some(terminal);
                        continue;
                    }
                    }
                    if emergency_chord.observe(keycode, pressed)
                        == EmergencyChordAction::Triggered
                    {
                        report.emergency_exit = true;
                        continue;
                    }
                    let decision = if routing_mode != PhysicalInputRoutingMode::CursorOnly {
                        shortcuts.as_deref_mut().map(|router|router.route_key(event.seat,keycode,pressed))
                    } else {None};
                    let switcher=decision.as_ref().is_some_and(|d|d.action.is_some_and(is_shell_switcher_shortcut));
                    let help=decision.as_ref().is_some_and(|d|d.action==Some(SHELL_HELP_SHORTCUT_ACTION));
                    if !switcher && !help && let Some((capture,keyboard))=launcher.as_mut() {
                        let (text,clear)=launcher_text.as_ref().map_or((None,false),|(text,clear)|(text.as_deref(),*clear));
                        let(consumed,input)=capture.route(&event,text,pointer.position(),clear,keyboard.command_modifier_active());
                        report.launcher_events.extend(input);
                        if consumed {key_repeat.cancel_seat(event.seat);continue;}
                    }
                    if !switcher && let Some(capture)=reference_capture.as_deref_mut() {
                        let (consumed,operation)=capture.route(&event);
                        report.reference_operations.extend(operation);
                        if consumed {key_repeat.cancel_seat(event.seat);continue;}
                    }
                    if let Some(decision)=decision && decision.consumed {
                        if pressed && key_repeat_map.evdev_key_repeats(keycode) {key_repeat.cancel_seat(event.seat);}
                        report.wm_actions.extend(decision.action);
                        report.policy_inputs.extend(decision.action.map(PhysicalPolicyInput::Action));
                        continue;
                    }
                }
                if !control_plane_applied
                    && !matches!(
                        routing_mode,
                        PhysicalInputRoutingMode::Full | PhysicalInputRoutingMode::ControlPlaneOnly
                    )
                {
                    continue;
                }
                if crate::input_proof::pointer_proof_suppresses_return(
                    pointer_proof_required,
                    keycode,
                    physical_text_proof
                        .as_deref()
                        .is_some_and(PhysicalTextProof::is_complete),
                ) {
                    report.return_suppressed = true;
                    continue;
                }
                let event = if control_plane_applied {
                    let Some(target) = event.target_surface else {
                        continue;
                    };
                    if focus.focused_surface(event.seat) != Some(target)
                        || !committed_surfaces
                            .iter()
                            .any(|committed| committed.surface == target)
                    {
                        continue;
                    }
                    event
                } else {
                    match focus.route_keyboard_event(event, committed_surfaces) {
                        FocusedInputRoute::Routed(event) => event,
                        FocusedInputRoute::NoFocus(_) => {
                            report.keys_suppressed_no_focus =
                                report.keys_suppressed_no_focus.saturating_add(1);
                            continue;
                        }
                        FocusedInputRoute::StaleFocus(_) => {
                            report.keys_suppressed_stale_focus =
                                report.keys_suppressed_stale_focus.saturating_add(1);
                            continue;
                        }
                        FocusedInputRoute::UnsupportedEvent(_) => continue,
                    }
                };
                let Some(target_surface) = event.target_surface else {
                    continue;
                };
                if !control_plane_applied
                    && routing_mode == PhysicalInputRoutingMode::ControlPlaneOnly
                {
                    if let Some(handoff) = keyboard_focus_handoff.as_deref_mut() {
                        let changed_target = handoff
                            .target()
                            .is_some_and(|held| held != target_surface);
                        let deferred_press =
                            pressed.then_some((event.serial, event.time_msec));
                        if handoff.defer(target_surface, now_msec, event).is_err() {
                            if changed_target {
                                report.keyboard_focus_handoff_stale_drops = report
                                    .keyboard_focus_handoff_stale_drops
                                    .saturating_add(1);
                            } else {
                                report.keyboard_focus_handoff_capacity_drops = report
                                    .keyboard_focus_handoff_capacity_drops
                                    .saturating_add(1);
                            }
                        } else if let Some(deferred_press) = deferred_press {
                            report.deferred_key_presses.push(deferred_press);
                        }
                    }
                    continue;
                }
                let key = SessionClientPressedKey {
                    surface: target_surface,
                    seat: event.seat,
                    device: event.device,
                    keycode,
                };
                if !pressed && !client_keys.release_is_routable(key) {
                    client_keys.record_routed(key, false);
                    continue;
                }
                let evdev_keycode = keycode;
                if !pressed {
                    let _ = key_repeat.release(event.seat, event.device, evdev_keycode);
                }
                let Some((keycode, state)) = modifiers.map_evdev_key(keycode, pressed) else {
                    continue;
                };
                if !crate::input_proof::physical_text_proof_ignores_evdev_key(evdev_keycode)
                    && let Some(proof) = physical_text_proof.as_deref_mut()
                    && !proof.is_complete() {
                        let observed = PhysicalTextProofEvent {
                            keycode,
                            pressed,
                            state,
                        };
                        if let Err(mismatch) = proof.observe(observed) {
                            return Err(format!(
                            "physical text proof sequence mismatch at event {}: expected keycode={} pressed={} state={} observed keycode={} pressed={} state={}",
                            mismatch.event_index,
                            mismatch.expected.keycode,
                            mismatch.expected.pressed,
                            mismatch.expected.state,
                            mismatch.observed.keycode,
                            mismatch.observed.pressed,
                            mismatch.observed.state,
                        )
                        .into());
                        }
                    }
                let delivery = XAuthorityInputDeliveryId::from_raw(*next_input_delivery);
                *next_input_delivery = next_input_delivery
                    .checked_add(1)
                    .ok_or("live-session input delivery ID exhausted")?;
                if !route_bounded_input(
                    input_sender,
                    XAuthorityRoutedInput {
                        request: sophia_protocol::RoutedInputRequest {
                            serial: event.serial,
                            seat: event.seat,
                            device: event.device,
                            time_msec: event.time_msec,
                            target_surface,
                            global_position: Point::default(),
                            local_position: Point::default(),
                            kind: event.kind,
                        },
                        route_lease: None,
                        delivery: Some(delivery),
                        mode: XAuthorityRoutedInputMode::Deliver,
                    },
                    if pressed {
                        sophia_protocol::CapacityClass::Ordered
                    } else {
                        sophia_protocol::CapacityClass::TerminatingBoundary
                    },
                    &mut report.ingress_saturation,
                )? {
                    continue;
                }
                if client_keys.record_routed(key, pressed).is_saturated() {
                    report.ingress_saturation.ledger_discarded =
                        report.ingress_saturation.ledger_discarded.saturating_add(1);
                    continue;
                }
                if pressed {
                    match key_repeat.arm(
                        KeyRepeatTarget {
                            surface: target_surface,
                            seat: event.seat,
                            device: event.device,
                            keycode: evdev_keycode,
                            source_time_msec: event.time_msec,
                        },
                        now_msec,
                        key_repeat_map.evdev_key_repeats(evdev_keycode),
                    ) {
                        sophia_engine::KeyRepeatArmOutcome::Armed
                        | sophia_engine::KeyRepeatArmOutcome::NotRepeatable => {}
                        sophia_engine::KeyRepeatArmOutcome::SeatCapacityExhausted => {
                            return Err("key repeat seat capacity exhausted".into());
                        }
                    }
                }
                report.keys_routed = report.keys_routed.saturating_add(1);
                report.key_targets.push(target_surface);
                if pressed {
                    report
                        .routed_key_presses
                        .push((event.serial, event.time_msec));
                }
                report.deliveries.push(delivery);
            }
            kind @ (sophia_protocol::InputEventKind::PointerMotion
            | sophia_protocol::InputEventKind::PointerButton { .. }
            | sophia_protocol::InputEventKind::PointerAxis { .. }) => {
                let is_button =
                    matches!(kind, sophia_protocol::InputEventKind::PointerButton { .. });
                let is_axis =
                    matches!(kind, sophia_protocol::InputEventKind::PointerAxis { .. });
                // Modal capture prevents application delivery, but the owner
                // still needs observed motion to update the visible cursor.
                report.pointer_events = report.pointer_events.saturating_add(1);
                if is_button {
                    report.pointer_buttons_observed =
                        report.pointer_buttons_observed.saturating_add(1);
                }
                if is_axis {
                    report.pointer_axes_observed =
                        report.pointer_axes_observed.saturating_add(1);
                }
                if !control_plane_applied && let Some((capture,_))=launcher.as_mut() {
                    if capture.active() && matches!(kind,sophia_protocol::InputEventKind::PointerMotion){
                        let focused=focus.focused_surface(event.seat);
                        let (_, placement)=place_pointer_event_for_routing(&mut event,focused,input_layers,pointer,false);
                        record_pointer_boundary_placement(&mut report, kind, placement);
                    }
                    let(consumed,input)=capture.route(&event,None,pointer.position(),false,false);
                    report.launcher_events.extend(input);
                    if consumed{continue;}
                }
                if !control_plane_applied && let Some(capture)=reference_capture.as_deref_mut() {
                    let (consumed,operation)=capture.route(&event);
                    report.reference_operations.extend(operation);
                    if consumed {
                        if matches!(kind,sophia_protocol::InputEventKind::PointerMotion) {
                            let focused=focus.focused_surface(event.seat);
                            let (_, placement)=place_pointer_event_for_routing(&mut event,focused,input_layers,pointer,false);
                            record_pointer_boundary_placement(&mut report, kind, placement);
                        }
                        continue;
                    }
                }
                if matches!(
                    routing_mode,
                    PhysicalInputRoutingMode::Suppressed | PhysicalInputRoutingMode::ShortcutsOnly
                ) {
                    if is_button {
                        report.pointer_buttons_suppressed_by_policy = report
                            .pointer_buttons_suppressed_by_policy
                            .saturating_add(1);
                    }
                    continue;
                }
                if matches!(
                    routing_mode,
                    PhysicalInputRoutingMode::CursorOnly
                        | PhysicalInputRoutingMode::ControlPlaneOnly
                ) {
                    if is_button {
                        report.pointer_buttons_suppressed_by_policy = report
                            .pointer_buttons_suppressed_by_policy
                            .saturating_add(1);
                    }
                    if !is_button {
                        let focused_surface = focus.focused_surface(event.seat);
                        let (_, placement) = place_pointer_event_for_routing(
                            &mut event,
                            focused_surface,
                            input_layers,
                            pointer,
                            false,
                        );
                        record_pointer_boundary_placement(&mut report, kind, placement);
                    }
                    continue;
                }
                let focused_surface = focus.focused_surface(event.seat);
                let (route_event, placement) = place_pointer_event_for_routing(
                    &mut event,
                    focused_surface,
                    input_layers,
                    pointer,
                    pointer_buttons_only,
                );
                record_pointer_boundary_placement(&mut report, kind, placement);
                if !route_event {
                    continue;
                }
                let output_index = placement
                    .and_then(|placement| placement.output_index)
                    .or_else(|| pointer.output_index());
                let (
                    input_layers,
                    chrome_targets,
                    chrome_occlusion,
                    descriptor_targets,
                    descriptor_occlusion,
                    content_binding,
                    input_output,
                    input_presentation_epoch,
                ) =
                    input_projection_for_pointer(
                        input_projections,
                        pointer_outputs,
                        output_index,
                        input_layers,
                        input_output,
                        input_presentation_epoch,
                    );
                let descriptor_occlusion = descriptor_occlusion.or_else(|| {
                    input_projections.into_iter().flatten().filter(|p|Some(p.output)==input_output).flat_map(|p|p.tab_occlusions.iter()).find(|r| {
                        event.global_position.is_some_and(|p| p.x >= f64::from(r.x) && p.y >= f64::from(r.y) && p.x < f64::from(r.x)+f64::from(r.width) && p.y < f64::from(r.y)+f64::from(r.height) && !input_layers.iter().any(|l| p.x >= f64::from(l.geometry.x) && p.y >= f64::from(l.geometry.y) && p.x < f64::from(l.geometry.x)+f64::from(l.geometry.width) && p.y < f64::from(l.geometry.y)+f64::from(l.geometry.height)))
                    }).copied()
                });
                let application_owned = application_route_leases
                    .as_deref()
                    .and_then(|state| state.lease(event.seat))
                    .is_some()
                    || pointer_focus_handoff
                        .as_deref()
                        .and_then(PointerFocusHandoffState::target)
                        .is_some();
                if pointer_routing_enabled
                    && let Some(state) = descriptor_captures.as_deref_mut()
                {
                    let disposition = sophia_engine::resolve_presented_chrome_pointer_event(
                        state,
                        event.seat,
                        event.device,
                        kind,
                        event.global_position,
                        input_output,
                        input_presentation_epoch,
                        descriptor_targets,
                        descriptor_occlusion,
                        application_owned,
                    )
                    .map_err(|error| {
                        format!("failed to route descriptor switcher input: {error:?}")
                    })?;
                    match disposition {
                        sophia_engine::PresentedChromePointerDisposition::Pass => {}
                        sophia_engine::PresentedChromePointerDisposition::Captured => {
                            report.chrome_captures_started =
                                report.chrome_captures_started.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            continue;
                        }
                        sophia_engine::PresentedChromePointerDisposition::Activated {
                            action,
                            activation,
                        } => {
                            report.chrome_actions_activated =
                                report.chrome_actions_activated.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            report.descriptor_activations.push((action, activation));
                            continue;
                        }
                        sophia_engine::PresentedChromePointerDisposition::Cancelled => {
                            report.chrome_captures_cancelled =
                                report.chrome_captures_cancelled.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            continue;
                        }
                        sophia_engine::PresentedChromePointerDisposition::Consumed => {
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            continue;
                        }
                    }
                }
                if pointer_routing_enabled
                    && let Some(state) = chrome_captures.as_deref_mut()
                {
                    let disposition = sophia_engine::resolve_chrome_pointer_event(
                        state,
                        event.seat,
                        event.device,
                        kind,
                        event.global_position,
                        input_output,
                        input_presentation_epoch,
                        chrome_targets,
                        chrome_occlusion,
                        application_owned,
                    )
                    .map_err(|error| format!("failed to route indicator input: {error:?}"))?;
                    match disposition {
                        sophia_engine::ChromePointerDisposition::Pass => {}
                        sophia_engine::ChromePointerDisposition::Captured => {
                            report.chrome_captures_started =
                                report.chrome_captures_started.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            continue;
                        }
                        sophia_engine::ChromePointerDisposition::Activated { output, action } => {
                            report.chrome_actions_activated =
                                report.chrome_actions_activated.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            report.chrome_activations.push((output, action));
                            continue;
                        }
                        sophia_engine::ChromePointerDisposition::Cancelled => {
                            report.chrome_captures_cancelled =
                                report.chrome_captures_cancelled.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            continue;
                        }
                        sophia_engine::ChromePointerDisposition::Consumed => {
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            continue;
                        }
                    }
                }
                if pointer_routing_enabled
                    && let Some(state) = content_captures.as_deref_mut()
                {
                    let disposition = sophia_engine::resolve_content_pointer_event(
                        state,
                        event.seat,
                        event.device,
                        kind,
                        event.global_position,
                        content_binding,
                        application_owned,
                    );
                    if let (sophia_protocol::InputEventKind::PointerButton { pressed, .. }, Some(binding)) = (kind, content_binding)
                        && !matches!(disposition, sophia_engine::ContentPointerDisposition::Pass)
                    {
                        let status = match &disposition {
                            sophia_engine::ContentPointerDisposition::Captured => "captured",
                            sophia_engine::ContentPointerDisposition::Activated(_) => "activated",
                            sophia_engine::ContentPointerDisposition::Cancelled => "cancelled",
                            _ => "consumed",
                        };
                        crate::session_println!(
                            "sophia_shell_pointer_binding schema=1 status={} pressed={} observed_output={} output_generation={} candidate={} presentation={} layout_generation={} authority_current={}",
                            status, pressed, binding.output.id, binding.output.generation,
                            binding.candidate_generation, binding.presentation_epoch,
                            binding.transform.layout_generation, binding.authority_current,
                        );
                    }
                    match disposition {
                        sophia_engine::ContentPointerDisposition::Pass => {}
                        sophia_engine::ContentPointerDisposition::Activated(target) => {
                            report.content_activations.push(target);
                            report.chrome_actions_activated =
                                report.chrome_actions_activated.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            continue;
                        }
                        sophia_engine::ContentPointerDisposition::Captured => {
                            report.chrome_captures_started =
                                report.chrome_captures_started.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            continue;
                        }
                        sophia_engine::ContentPointerDisposition::Cancelled
                        | sophia_engine::ContentPointerDisposition::Consumed => {
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            continue;
                        }
                    }
                }
                if let Some(gesture) = floating_gesture.as_deref_mut() {
                    let position = event.global_position.map(|global| {
                        sophia_protocol::WmPointerPosition {
                            x: global.x.round() as i32,
                            y: global.y.round() as i32,
                        }
                    });
                    let super_held = shortcuts.as_deref().is_some_and(|shortcuts| {
                        shortcuts.modifier_mask(event.seat).bits
                            & sophia_protocol::WmModifierMask::SUPER
                            != 0
                    });
                    let route =
                        sophia_engine::hit_test_scene_surface_for_input(&event, input_layers);
                    let observation = observe_floating_pointer_gesture(
                        gesture,
                        kind,
                        position,
                        route.target_surface,
                        route
                            .target_surface
                            .and_then(|surface| surface_roles.get(&surface).copied()),
                        route.target_surface.and_then(|surface| {
                            input_layers
                                .iter()
                                .find(|layer| layer.surface == surface)
                                .map(|layer| layer.geometry)
                        }),
                        super_held,
                    );
                    if let Some(completed) = observation.completed {
                        report.wm_pointer_gestures.push(completed);
                    }
                    if let Some(interaction) = observation.interaction {
                        report.wm_pointer_interactions.push(interaction);
                    }
                    if observation.outline != FloatingPointerOutlineUpdate::Unchanged {
                        report.floating_outline = observation.outline;
                    }
                    if observation.consumed {
                        continue;
                    }
                }
                if !pointer_routing_enabled {
                    if is_button {
                        report.pointer_buttons_suppressed_by_policy = report
                            .pointer_buttons_suppressed_by_policy
                            .saturating_add(1);
                    }
                    continue;
                }
                // Capacity failure clears the entire held sequence. Suppress
                // the rest of this already-polled pointer batch as part of
                // the same atomic drop; fresh input resumes next owner turn.
                if report.pointer_focus_handoff_capacity_drops != 0 {
                    continue;
                }
                let pending_target = pointer_focus_handoff
                    .as_deref()
                    .and_then(PointerFocusHandoffState::target);
                let held_lease = application_route_leases
                    .as_deref()
                    .and_then(|state| state.lease(event.seat));
                let tab_occlusions = input_projections.into_iter().flatten()
                    .find(|projection| Some(projection.output) == input_output)
                    .map_or(&[][..], |projection| projection.tab_occlusions.as_slice());
                let route = if let Some(mut lease) = held_lease {
                    let Some(state) = application_route_leases.as_deref_mut() else { unreachable!() };
                    if state.routing_readiness(event.seat) == Some(sophia_engine::ApplicationRouteLeaseReadiness::Releasing) {
                        report.pointer_lease_waits += 1;
                        continue;
                    }
                    let scope = presented_application_scope(&event, input_layers, chrome_targets, chrome_occlusion,
                        descriptor_targets, descriptor_occlusion, tab_occlusions, client_routes);
                    let owner = client_routes.admission_for_surface(lease.target_surface);
                    let authorized_scope = scope.is_some_and(|scope| lease.scope.covers(scope))
                        && owner.is_some_and(|owner| owner.client_id == lease.admission
                            && owner.auth_provenance.session_generation == lease.authority_session_epoch)
                        && lease.identity.control_epoch == state.control_epoch()
                        && lease.initiating_device.is_none_or(|device| device == event.device);
                    let pinned = input_output.is_some_and(|output| match lease.binding() {
                        sophia_engine::ApplicationRouteLeaseBinding::Bound { output: bound, .. } => bound == output,
                        sophia_engine::ApplicationRouteLeaseBinding::AwaitingPresentation { .. } =>
                            state.pin_output(lease.identity, output).is_ok(),
                    });
                    if !authorized_scope || !pinned {
                        report.pointer_lease_rejections += 1;
                        record_application_lease_refusal(if !pinned { "output" } else { "outside_scope" });
                        if let Some(sender) = route_lease_release_sender {
                            if let Some(held) = pending_lease_input.as_deref_mut() {
                                cancel_application_lease(state, client_routes, sender, held, lease.identity, now_msec)?;
                            } else {
                                request_application_route_lease_release(state, client_routes, sender, event.seat, now_msec)?;
                            }
                        }
                        continue;
                    }
                    let output = input_output.expect("validated output");
                    lease = state.lease(event.seat).expect("pinned current lease");
                    if matches!(lease.binding(), sophia_engine::ApplicationRouteLeaseBinding::AwaitingPresentation { .. })
                        && sophia_engine::scene_contains_input_surface(input_layers, lease.target_surface)
                    {
                        lease = state.bind_presentation(lease.identity, output, input_presentation_epoch)
                            .map_err(|error| format!("failed to bind presented input: {error:?}"))?;
                    }
                    match state.routing_readiness(event.seat) {
                        Some(sophia_engine::ApplicationRouteLeaseReadiness::WaitForActivation
                            | sophia_engine::ApplicationRouteLeaseReadiness::WaitForPresentation) => {
                            report.pointer_lease_waits += 1;
                            if let Some(held) = pending_lease_input.as_deref_mut()
                                && let Err(error) = held.defer(lease, output, now_msec, event)
                                && let Some(sender) = route_lease_release_sender
                            {
                                record_application_lease_refusal(match error {
                                    HeldLeaseInputError::OutputChanged => "output",
                                    HeldLeaseInputError::Expired => "binding_timeout",
                                    HeldLeaseInputError::Capacity => "capacity",
                                });
                                cancel_application_lease(state, client_routes, sender, held, lease.identity, now_msec)?;
                                report.pointer_focus_handoff_capacity_drops += 1;
                            }
                            continue;
                        }
                        Some(sophia_engine::ApplicationRouteLeaseReadiness::ReadyForEvidenceValidation) => {}
                        _ => { report.pointer_lease_waits += 1; continue; }
                    }
                    if let Err(reason) = authorize_presented_lease(state, lease, &event, client_routes,
                        scope.expect("validated scope"), output, input_presentation_epoch, input_layers)
                    {
                        report.pointer_lease_rejections += 1;
                        record_application_lease_refusal(application_lease_refusal_reason(reason));
                        if let Some(sender) = route_lease_release_sender {
                            if let Some(held) = pending_lease_input.as_deref_mut() {
                                cancel_application_lease(state, client_routes, sender, held, lease.identity, now_msec)?;
                            } else {
                                request_application_route_lease_release(state, client_routes, sender, event.seat, now_msec)?;
                            }
                        }
                        continue;
                    }
                    sophia_engine::route_scene_surface_for_input(&event, input_layers, lease.target_surface)
                } else if let Some(target) = pending_target {
                    sophia_engine::route_scene_surface_for_input(&event, input_layers, target)
                } else {
                    sophia_engine::hit_test_scene_surface_for_input(&event, input_layers)
                };
                if matches!(kind, sophia_protocol::InputEventKind::PointerMotion)
                    && held_lease.is_none() && pending_target.is_none()
                    && input_presentation_epoch != 0
                    && let (Some(output), Some(position)) = (input_output, event.global_position)
                {
                    let occluded = chrome_occlusion.into_iter().chain(descriptor_occlusion)
                        .chain(tab_occlusions.iter().copied()).any(|r| point_is_inside_rect(position, r));
                    let popup = route.target_surface.is_some_and(|surface|
                        surface_roles.get(&surface) == Some(&sophia_protocol::SurfacePresentationRole::ClientPositioned));
                    if !occluded && !popup {
                        report.policy_inputs.push(PhysicalPolicyInput::Hover(PresentedPointerFocus {
                            output, target: route.target_surface,
                        }));
                    }
                }
                if is_button && route.target_surface.is_none() {
                    report.pointer_buttons_suppressed_no_target = report
                        .pointer_buttons_suppressed_no_target
                        .saturating_add(1);
                }
                let (Some(global), Some(local)) = (event.global_position, route.local_position)
                else {
                    continue;
                };
                let Some(surface) = route.target_surface else {
                    continue;
                };
                let focus_surface = pointer_focus_surface(
                    surface,
                    global,
                    input_layers,
                    surface_roles,
                    client_routes,
                );
                let request = sophia_protocol::RoutedInputRequest {
                    serial: event.serial,
                    seat: event.seat,
                    device: event.device,
                    time_msec: event.time_msec,
                    target_surface: surface,
                    global_position: global,
                    local_position: local,
                    kind,
                };
                let starts_focus_handoff = held_lease.is_none() && pointer_press_starts_focus_handoff(
                    &kind,
                    applied_client_focus,
                    focus_surface,
                    surface_roles.get(&focus_surface).copied(),
                    pointer_focus_handoff
                        .as_deref()
                        .is_some_and(|handoff| handoff.target().is_none()),
                );
                if let Some(handoff) = pointer_focus_handoff.as_deref_mut() {
                    if starts_focus_handoff {
                        handoff.begin(focus_surface, now_msec, request)?;
                        report.pointer_focus_targets.push(focus_surface);
                        report.policy_inputs.push(PhysicalPolicyInput::ClickFocus(focus_surface));
                        continue;
                    }
                    if handoff.target().is_some() {
                        if handoff.defer(request).is_err() {
                            report.pointer_focus_handoff_capacity_drops = report
                                .pointer_focus_handoff_capacity_drops
                                .saturating_add(1);
                        }
                        continue;
                    }
                }
                let delivery = XAuthorityInputDeliveryId::from_raw(*next_input_delivery);
                *next_input_delivery = next_input_delivery
                    .checked_add(1)
                    .ok_or("live-session input delivery ID exhausted")?;
                let route_lease = match application_route_leases.as_deref_mut() {
                    Some(state) => application_route_lease_for_request(
                        &request,
                        client_routes,
                        state,
                        input_output,
                        input_presentation_epoch,
                    )?,
                    None => None,
                };
                if !route_bounded_input(
                    input_sender,
                    XAuthorityRoutedInput {
                        request,
                        route_lease,
                        delivery: Some(delivery),
                        mode: XAuthorityRoutedInputMode::Deliver,
                    },
                    sophia_protocol::CapacityClass::Ordered,
                    &mut report.ingress_saturation,
                )? {
                    continue;
                }
                report.pointer_routed = report.pointer_routed.saturating_add(1);
                if is_button {
                    report.pointer_buttons_routed = report.pointer_buttons_routed.saturating_add(1);
                    report.pointer_button_targets.push(surface);
                }
                if is_axis {
                    report.pointer_axes_routed = report.pointer_axes_routed.saturating_add(1);
                    report.pointer_axis_targets.push(surface);
                }
                report.deliveries.push(delivery);
            }
        }
    }
    Ok(report)
}

fn pointer_focus_surface(
    target: SurfaceId,
    global: sophia_protocol::Point,
    input_layers: &[LayerSnapshot],
    surface_roles: &BTreeMap<SurfaceId, sophia_protocol::SurfacePresentationRole>,
    client_routes: &XAuthorityClientSurfaceRoutes,
) -> SurfaceId {
    if surface_roles.get(&target)
        != Some(&sophia_protocol::SurfacePresentationRole::ClientPositioned)
    {
        return target;
    }
    let Some(client) = client_routes.client_for_surface(target) else {
        return target;
    };
    input_layers
        .iter()
        .filter(|layer| {
            surface_roles.get(&layer.surface)
                == Some(&sophia_protocol::SurfacePresentationRole::PolicyManaged)
                && client_routes.client_for_surface(layer.surface) == Some(client)
                && point_is_inside_rect(global, layer.geometry)
        })
        .max_by_key(|layer| (layer.stack_rank, layer.surface))
        .map_or(target, |layer| layer.surface)
}

fn point_is_inside_rect(point: sophia_protocol::Point, rect: sophia_protocol::Rect) -> bool {
    point.x >= f64::from(rect.x)
        && point.y >= f64::from(rect.y)
        && point.x < f64::from(rect.x.saturating_add(rect.width))
        && point.y < f64::from(rect.y.saturating_add(rect.height))
}

fn pointer_press_starts_focus_handoff(
    kind: &sophia_protocol::InputEventKind,
    applied_focus: Option<SurfaceId>,
    target: SurfaceId,
    role: Option<sophia_protocol::SurfacePresentationRole>,
    handoff_idle: bool,
) -> bool {
    matches!(
        kind,
        sophia_protocol::InputEventKind::PointerButton {
            button: 0x110,
            pressed: true
        }
    ) && applied_focus != Some(target)
        && role != Some(sophia_protocol::SurfacePresentationRole::ClientPositioned)
        && handoff_idle
}

fn record_pointer_boundary_placement(
    report: &mut PhysicalInputRouteReport,
    kind: sophia_protocol::InputEventKind,
    placement: Option<sophia_engine::OutputUnionPointerPlacement>,
) {
    if !matches!(kind, sophia_protocol::InputEventKind::PointerMotion) {
        return;
    }
    let Some(placement) = placement else {
        return;
    };
    if !placement.entered.is_empty() {
        report
            .pointer_boundary_entries
            .push((placement.entered, placement.output_index));
    }
    if !placement.reversed.is_empty() {
        report
            .pointer_boundary_reversals
            .push((placement.reversed, placement.output_index));
    }
    if let Some(transition) = placement.transition {
        report
            .pointer_output_transitions
            .push((transition, placement.contact.is_empty()));
    }
}
