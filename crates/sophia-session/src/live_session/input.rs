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
#[path = "input/route_report.rs"]
mod route_report;
use route_report::*;
#[path = "input/grab_routing.rs"]
mod grab_routing;
use grab_routing::*;
#[path = "input/device_lifecycle.rs"]
mod device_lifecycle;
use device_lifecycle::*;

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
    &'a [sophia_engine::PresentedContentBinding],
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
        return (&[], &[], None, &[], None, &[], Some(output.id), 0);
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
                &[],
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
                    projection.content.as_slice(),
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
    /// Whether anything exists that can CLOSE an ordered focus handoff.
    /// A press that opens one is withheld until the handoff answers, so
    /// where nothing answers the press expires with it and the button is
    /// never delivered. Only a WM session answers, and a session may run
    /// without one -- the QEMU pointer proof does -- so the handoff is
    /// offered only when it can be completed.
    pointer_focus_policy_available: bool,
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
    /// Holds pointer motion so it reaches the frontend at the composition
    /// cadence instead of once per event. The owner loop owns it, so motion
    /// buffered by one pass survives into the next.
    routed_input_coalescer: &'a mut sophia_engine::RoutedInputCoalescer,
    /// Whether the frame pacer says a repaint is due, which releases buffered
    /// motion.
    repaint_due: bool,
    /// When the motion now buffered was first held, so it can be released on
    /// the cadence even when no paced repaint asks for it.
    motion_held_since: &'a mut Option<std::time::Instant>,
    /// One composed frame, the longest buffered motion may wait.
    frame_interval: std::time::Duration,
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
        pointer_focus_policy_available,
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
        routed_input_coalescer,
        repaint_due,
        motion_held_since,
        frame_interval,
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
        pointer_focus_policy_available.then_some(pointer_focus_handoff),
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
        routed_input_coalescer,
        repaint_due,
        motion_held_since,
        frame_interval,
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
    // This path routes one batch and returns, so the end of that batch is its
    // frame boundary and its motion is released there.
    let mut routed_input_coalescer = sophia_engine::RoutedInputCoalescer::new();
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
        &mut routed_input_coalescer,
        true,
        &mut None,
        std::time::Duration::ZERO,
    )
}

include!("input/routing.rs");

/// Deliver, in order, the inputs a coalescer released.
///
/// This is the tail the per-event path used to run inline: mint a delivery id,
/// take a route lease, send, and account for what was sent. It runs once per
/// released packet rather than once per packet observed.
#[allow(clippy::too_many_arguments)]
fn deliver_coalesced_inputs(
    flush: sophia_engine::RoutedInputFlush,
    input_sender: &impl RoutedInputIngress,
    next_input_delivery: &mut u64,
    mut application_route_leases: Option<&mut ApplicationRouteLeaseState>,
    client_routes: &XAuthorityClientSurfaceRoutes,
    input_output: Option<sophia_protocol::OutputId>,
    input_presentation_epoch: u64,
    report: &mut PhysicalInputRouteReport,
) -> Result<(), Box<dyn std::error::Error>> {
    for queued in flush.inputs {
        // The route was restated as routed, with a valid surface and a local
        // position, before it was buffered, so a refusal here cannot describe a
        // packet this path would have delivered.
        let Ok(request) = sophia_engine::routed_input_request_from_physical_event(
            &queued.event,
            &queued.route,
        ) else {
            continue;
        };
        let surface = request.target_surface;
        let is_button = matches!(
            request.kind,
            sophia_protocol::InputEventKind::PointerButton { .. }
        );
        let is_axis = matches!(
            request.kind,
            sophia_protocol::InputEventKind::PointerAxis { .. }
        );
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
                origin: sophia_x_authority::XAuthorityRoutedInputOrigin::Physical,
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
    Ok(())
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

include!("input/lease_service.rs");
