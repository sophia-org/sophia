use super::super::{InputDeliveryPhase, InputDeliveryState};
use super::*;
use crate::live_session::{
    ExplicitPointerGrabQueue, FloatingPointerPolicyInteraction, PendingLeaseInput,
    PersistentLiveLayout, RoutedInputIngressSaturation, drain_explicit_pointer_grab_controls,
    pointer_focus_surface,
};
use sophia_engine::{ApplicationRouteLeasePhase, ApplicationRouteLeaseState, InputFocusDecision};
use sophia_protocol::TransactionId;
use sophia_x_authority::{
    XAuthorityClientInputDelivery, XAuthorityInputDeliveryId, XAuthorityInputDeliveryOutcome,
};
use std::collections::{BTreeMap, BTreeSet};

include!("input_policy_tests/pointer_gestures.rs");

#[test]
fn flushed_input_delivery_retires_its_client_key_release_barrier() {
    let delivery = XAuthorityInputDeliveryId::from_raw(7);
    let mut state = InputDeliveryState::default();
    state
        .pending
        .insert(delivery, pending_delivery_fixture(delivery));
    state.events_expected = 1;
    let mut release_barrier = BTreeSet::from([delivery]);
    let (sender, receiver) = sync_channel(1);
    sender
        .send(XAuthorityClientInputDelivery {
            client: sophia_x_authority::XServerFrontendClientId::from_raw(1),
            delivery,
            outcome: XAuthorityInputDeliveryOutcome::Flushed,
        })
        .unwrap();
    let mut proof_started_at = None;
    let mut post_input_deadline = None;

    InputDeliveryPhase {
        sender: None,
        receiver: &receiver,
        state: &mut state,
        client_key_release_barrier: &mut release_barrier,
        proof_started_at: &mut proof_started_at,
        post_input_deadline: &mut post_input_deadline,
    }
    .drain()
    .unwrap();

    assert!(state.pending.is_empty());
    assert!(release_barrier.is_empty());
    assert_eq!(state.events_flushed, 1);
}

#[test]
fn target_gone_delivery_retires_without_poisoning_the_session() {
    let delivery = XAuthorityInputDeliveryId::from_raw(8);
    let mut state = InputDeliveryState::default();
    state
        .pending
        .insert(delivery, pending_delivery_fixture(delivery));
    state.events_expected = 1;
    let mut release_barrier = BTreeSet::from([delivery]);
    let (sender, receiver) = sync_channel(1);
    sender
        .send(XAuthorityClientInputDelivery {
            client: sophia_x_authority::XServerFrontendClientId::from_raw(1),
            delivery,
            outcome: XAuthorityInputDeliveryOutcome::TargetGone,
        })
        .unwrap();
    let mut proof_started_at = None;
    let mut post_input_deadline = None;

    InputDeliveryPhase {
        sender: None,
        receiver: &receiver,
        state: &mut state,
        client_key_release_barrier: &mut release_barrier,
        proof_started_at: &mut proof_started_at,
        post_input_deadline: &mut post_input_deadline,
    }
    .drain()
    .unwrap();

    assert!(state.pending.is_empty());
    assert!(release_barrier.is_empty());
    assert_eq!(state.events_expected, 0);
    assert_eq!(state.events_flushed, 0);
}

/// A boundary the session drew itself does not end the session.
///
/// Closing the input epoch for an output policy change revokes whatever was in
/// flight. A live run died mid-topology because the pointer moved while that
/// happened and the revocation was read as a delivery fault.
#[test]
fn epoch_revoked_delivery_retires_without_poisoning_the_session() {
    let delivery = XAuthorityInputDeliveryId::from_raw(9);
    let mut state = InputDeliveryState::default();
    state
        .pending
        .insert(delivery, pending_delivery_fixture(delivery));
    state.events_expected = 1;
    let mut release_barrier = BTreeSet::from([delivery]);
    let (sender, receiver) = sync_channel(1);
    sender
        .send(XAuthorityClientInputDelivery {
            client: sophia_x_authority::XServerFrontendClientId::from_raw(1),
            delivery,
            outcome: XAuthorityInputDeliveryOutcome::EpochRevoked,
        })
        .unwrap();
    let mut proof_started_at = None;
    let mut post_input_deadline = None;

    InputDeliveryPhase {
        sender: None,
        receiver: &receiver,
        state: &mut state,
        client_key_release_barrier: &mut release_barrier,
        proof_started_at: &mut proof_started_at,
        post_input_deadline: &mut post_input_deadline,
    }
    .drain()
    .unwrap();

    assert!(state.pending.is_empty());
    assert!(release_barrier.is_empty());
    assert_eq!(state.events_expected, 0);
    assert_eq!(state.events_flushed, 0);
}

/// A route that genuinely could not be delivered still ends the session.
#[test]
fn route_rejected_delivery_remains_fatal() {
    let delivery = XAuthorityInputDeliveryId::from_raw(10);
    let mut state = InputDeliveryState::default();
    state
        .pending
        .insert(delivery, pending_delivery_fixture(delivery));
    state.events_expected = 1;
    let mut release_barrier = BTreeSet::from([delivery]);
    let (sender, receiver) = sync_channel(1);
    sender
        .send(XAuthorityClientInputDelivery {
            client: sophia_x_authority::XServerFrontendClientId::from_raw(1),
            delivery,
            outcome: XAuthorityInputDeliveryOutcome::RouteRejected,
        })
        .unwrap();
    let mut proof_started_at = None;
    let mut post_input_deadline = None;

    assert!(
        InputDeliveryPhase {
            sender: None,
            receiver: &receiver,
            state: &mut state,
            client_key_release_barrier: &mut release_barrier,
            proof_started_at: &mut proof_started_at,
            post_input_deadline: &mut post_input_deadline,
        }
        .drain()
        .is_err()
    );
}

#[test]
fn emergency_chord_flushes_routed_modifiers_before_shutdown() {
    let seat = SeatId::from_raw(1);
    let surface = SurfaceId::new(41, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 480,
    };
    let committed = [CommittedSurfaceState {
        surface,
        committed_generation: 1,
        geometry,
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 1 },
            sophia_protocol::Size {
                width: geometry.width,
                height: geometry.height,
            },
        ),
        damage: Region::single(geometry),
    }];
    let mut focus = InputFocusState::new();
    assert_eq!(
        focus.focus_surface(seat, surface, &committed),
        InputFocusDecision::Focused
    );
    let events = [29, 56, 14]
        .into_iter()
        .enumerate()
        .map(|(index, keycode)| InputEventPacket {
            serial: u64::try_from(index + 1).unwrap(),
            seat,
            device: DeviceId::from_raw(1),
            time_msec: u64::try_from(index + 1).unwrap(),
            kind: InputEventKind::Key {
                keycode,
                pressed: true,
            },
            global_position: None,
            target_surface: None,
            local_position: None,
        })
        .collect();
    let (input_sender, input_receiver) = sync_channel(8);
    let mut modifiers = XCoreKeyboardMapper::new();
    let (mut key_repeat, key_repeat_map) = test_key_repeat_parts();
    let mut client_keys = SessionClientKeyState::default();
    let mut emergency = super::super::EmergencyChordState::armed();
    let mut virtual_terminal = crate::session_keyboard::VirtualTerminalChordState::default();
    let mut keyboard_coverage = PhysicalKeyboardCoverage::default();
    let mut pointer = SessionPointerPlacement::default();
    let mut next_delivery = 1;

    let report = route_input_events(
        events,
        &focus,
        &committed,
        &[],
        &XAuthorityClientSurfaceRoutes::default(),
        &input_sender,
        &mut modifiers,
        &mut key_repeat,
        &key_repeat_map,
        &mut client_keys,
        &mut emergency,
        &mut virtual_terminal,
        &mut keyboard_coverage,
        None,
        &mut pointer,
        false,
        false,
        false,
        PhysicalInputRoutingMode::Full,
        &mut next_delivery,
        3,
        None,
        None,
        None,
    )
    .unwrap();
    let routed_presses = input_receiver.try_iter().collect::<Vec<_>>();

    assert!(report.emergency_exit);
    assert_eq!(report.keys_routed, 2);
    assert_eq!(routed_presses.len(), 2);
    assert_eq!(client_keys.pending_len(), 2);

    let mut scratch = Vec::new();
    let mut deliveries = Vec::new();
    let released = flush_all_client_pressed_keys(
        &mut client_keys,
        &mut scratch,
        &mut deliveries,
        &input_sender,
        &mut RoutedInputIngressSaturation::default(),
        &mut modifiers,
        &mut next_delivery,
        4,
    )
    .unwrap();
    let routed_releases = input_receiver
        .try_iter()
        .map(|input| input.request.kind)
        .collect::<Vec<_>>();

    assert_eq!(released, 2);
    assert_eq!(deliveries.len(), 2);
    assert_eq!(client_keys.pending_len(), 0);
    assert_eq!(modifiers.modifier_mask(), 0);
    assert_eq!(
        routed_releases,
        [
            InputEventKind::Key {
                keycode: 29,
                pressed: false,
            },
            InputEventKind::Key {
                keycode: 56,
                pressed: false,
            },
        ]
    );
}

#[test]
fn client_positioned_primary_press_bypasses_managed_focus_handoff() {
    let surface = SurfaceId::new(41, 1);
    let press = InputEventKind::PointerButton {
        button: 0x110,
        pressed: true,
    };
    assert!(!pointer_press_starts_focus_handoff(
        &press,
        Some(SurfaceId::new(42, 1)),
        surface,
        Some(sophia_protocol::SurfacePresentationRole::ClientPositioned),
        true,
    ));
    assert!(pointer_press_starts_focus_handoff(
        &press,
        Some(SurfaceId::new(42, 1)),
        surface,
        Some(sophia_protocol::SurfacePresentationRole::PolicyManaged),
        true,
    ));
}

#[test]
fn client_positioned_pointer_target_focuses_containing_managed_surface_for_same_client() {
    let managed = SurfaceId::new(41, 1);
    let child = SurfaceId::new(42, 1);
    let other = SurfaceId::new(43, 1);
    let client = sophia_x_authority::XServerFrontendClientId::from_raw(9);
    let other_client = sophia_x_authority::XServerFrontendClientId::from_raw(10);
    let geometry = Rect {
        x: 100,
        y: 50,
        width: 800,
        height: 600,
    };
    let layer = |surface, stack_rank| LayerSnapshot {
        input_region: None,
        translation: None,
        output: None,
        surface,
        authority_local_id: None,
        namespace: None,
        stack_rank,
        geometry,
        source_size: Size {
            width: geometry.width,
            height: geometry.height,
        },
        source: BufferSource::None,
        damage: Region::empty(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
    };
    let layers = [layer(managed, 2), layer(child, 3), layer(other, 4)];
    let roles = BTreeMap::from([
        (
            managed,
            sophia_protocol::SurfacePresentationRole::PolicyManaged,
        ),
        (
            child,
            sophia_protocol::SurfacePresentationRole::ClientPositioned,
        ),
        (
            other,
            sophia_protocol::SurfacePresentationRole::PolicyManaged,
        ),
    ]);
    let mut routes = XAuthorityClientSurfaceRoutes::default();
    for (surface, route_client) in [(managed, client), (child, client), (other, other_client)] {
        let mut batch = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(
            u64::from(surface.index()),
        ));
        batch.client = Some(route_client);
        batch
            .surface_routes
            .push(sophia_x_authority::XAuthoritySurfaceRouteObservation {
                surface,
                client: route_client,
                admission: None,
            });
        batch
            .presentation_intents
            .push(sophia_protocol::SurfacePresentationIntent {
                surface,
                kind: sophia_protocol::SurfacePresentationIntentKind::Request,
                role: roles[&surface],
                surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
                placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
                presentation_owner: None,
                stack_rank: 0,
                geometry,
                constraints: sophia_protocol::SurfaceConstraints {
                    min_size: None,
                    max_size: None,
                },
                generation: 1,
            });
        routes.observe(&batch).unwrap();
    }

    assert_eq!(
        pointer_focus_surface(child, Point { x: 120.0, y: 80.0 }, &layers, &roles, &routes,),
        managed,
    );
}

#[test]
fn unknown_surface_keeps_wm_focus_request_pending() {
    let request = (TransactionId::from_raw(7), SurfaceId::new(41, 1));
    assert_eq!(
        pending_wm_focus_after_engine_decision(request, InputFocusDecision::UnknownSurface),
        Some(request),
    );
    assert_eq!(
        pending_wm_focus_after_engine_decision(request, InputFocusDecision::Focused),
        None,
    );
    // An unchanged focus satisfies the request. Holding it pending would re-arm
    // the reconciliation every turn for a change that already happened.
    assert_eq!(
        pending_wm_focus_after_engine_decision(request, InputFocusDecision::AlreadyFocused),
        None,
    );
}

#[test]
fn held_application_pointer_delivery_does_not_freeze_cursor() {
    let action = WmActionId::from_raw(7);
    let registry = WmShortcutRegistry::new(
        &[WmBindingRegistration {
            action,
            keycode: 28,
            modifiers: WmModifierMask {
                bits: WmModifierMask::SUPER,
            },
        }],
        WmCapabilities::all_supported(),
        1,
        sophia_protocol::WmChromePolicy::default(),
    )
    .unwrap();
    let mut shortcuts = WmShortcutRouter::new(registry);
    let events = vec![
        InputEventPacket {
            serial: 1,
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(2),
            time_msec: 1,
            kind: InputEventKind::PointerMotion,
            global_position: Some(Point { x: 18.0, y: -5.0 }),
            target_surface: None,
            local_position: None,
        },
        InputEventPacket {
            serial: 2,
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(1),
            time_msec: 2,
            kind: InputEventKind::Key {
                keycode: 125,
                pressed: true,
            },
            global_position: None,
            target_surface: None,
            local_position: None,
        },
        InputEventPacket {
            serial: 3,
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(1),
            time_msec: 3,
            kind: InputEventKind::Key {
                keycode: 28,
                pressed: true,
            },
            global_position: None,
            target_surface: None,
            local_position: None,
        },
    ];
    let (input_sender, input_receiver) = sync_channel(1);
    let mut modifiers = XCoreKeyboardMapper::new();
    let (mut key_repeat, key_repeat_map) = super::test_key_repeat_parts();
    let mut client_keys = SessionClientKeyState::default();
    let mut emergency = super::super::EmergencyChordState::awaiting_arm();
    let mut virtual_terminal = crate::session_keyboard::VirtualTerminalChordState::default();
    let mut keyboard_coverage = PhysicalKeyboardCoverage::default();
    let mut pointer = SessionPointerPlacement::default();
    pointer.center_on_primary_output(Size {
        width: 2560,
        height: 1440,
    });
    let initial_position = pointer.position();
    let mut next_delivery = 1;

    let report = route_input_events(
        events,
        &InputFocusState::new(),
        &[],
        &[],
        &XAuthorityClientSurfaceRoutes::default(),
        &input_sender,
        &mut modifiers,
        &mut key_repeat,
        &key_repeat_map,
        &mut client_keys,
        &mut emergency,
        &mut virtual_terminal,
        &mut keyboard_coverage,
        Some(&mut shortcuts),
        &mut pointer,
        false,
        false,
        false,
        PhysicalInputRoutingMode::Full,
        &mut next_delivery,
        0,
        None,
        None,
        None,
    )
    .unwrap();

    assert_eq!(report.pointer_events, 1);
    assert_eq!(report.pointer_routed, 0);
    assert_eq!(report.wm_actions, [action]);
    assert_eq!(report.keys_routed, 0);
    assert_ne!(pointer.position(), initial_position);
    assert!(input_receiver.try_recv().is_err());
}

#[test]
fn full_routing_suppresses_keyboard_input_when_workspace_focus_is_clear() {
    let events = vec![InputEventPacket {
        serial: 1,
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(1),
        time_msec: 1,
        kind: InputEventKind::Key {
            keycode: 30,
            pressed: true,
        },
        global_position: None,
        target_surface: None,
        local_position: None,
    }];
    let (input_sender, input_receiver) = sync_channel(1);
    let mut modifiers = XCoreKeyboardMapper::new();
    let (mut key_repeat, key_repeat_map) = super::test_key_repeat_parts();
    let mut client_keys = SessionClientKeyState::default();
    let mut emergency = super::super::EmergencyChordState::awaiting_arm();
    let mut virtual_terminal = crate::session_keyboard::VirtualTerminalChordState::default();
    let mut keyboard_coverage = PhysicalKeyboardCoverage::default();
    let mut pointer = SessionPointerPlacement::default();
    let mut next_delivery = 1;

    let report = route_input_events(
        events,
        &InputFocusState::new(),
        &[],
        &[],
        &XAuthorityClientSurfaceRoutes::default(),
        &input_sender,
        &mut modifiers,
        &mut key_repeat,
        &key_repeat_map,
        &mut client_keys,
        &mut emergency,
        &mut virtual_terminal,
        &mut keyboard_coverage,
        None,
        &mut pointer,
        false,
        false,
        false,
        PhysicalInputRoutingMode::Full,
        &mut next_delivery,
        0,
        None,
        None,
        None,
    )
    .unwrap();

    assert_eq!(report.keys_suppressed_no_focus, 1);
    assert_eq!(report.keys_routed, 0);
    assert!(input_receiver.try_recv().is_err());
}

#[test]
fn keyboard_focus_handoff_preserves_client_text_until_frontend_focus_applies() {
    let seat = SeatId::from_raw(1);
    let surface = SurfaceId::new(41, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 480,
    };
    let committed = [CommittedSurfaceState {
        surface,
        committed_generation: 1,
        geometry,
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 1 },
            sophia_protocol::Size {
                width: geometry.width,
                height: geometry.height,
            },
        ),
        damage: Region::single(geometry),
    }];
    let mut focus = InputFocusState::new();
    assert_eq!(
        focus.focus_surface(seat, surface, &committed),
        InputFocusDecision::Focused
    );
    let events = [true, false]
        .into_iter()
        .enumerate()
        .map(|(index, pressed)| InputEventPacket {
            serial: u64::try_from(index + 1).unwrap(),
            seat,
            device: DeviceId::from_raw(1),
            time_msec: u64::try_from(index + 1).unwrap(),
            kind: InputEventKind::Key {
                keycode: 35,
                pressed,
            },
            global_position: None,
            target_surface: None,
            local_position: None,
        })
        .collect();
    let (input_sender, input_receiver) = sync_channel(4);
    let mut modifiers = XCoreKeyboardMapper::new();
    let (mut key_repeat, key_repeat_map) = super::test_key_repeat_parts();
    let mut client_keys = SessionClientKeyState::default();
    let mut emergency = super::super::EmergencyChordState::awaiting_arm();
    let mut virtual_terminal = crate::session_keyboard::VirtualTerminalChordState::default();
    let mut keyboard_coverage = PhysicalKeyboardCoverage::default();
    let mut pointer = SessionPointerPlacement::default();
    let mut next_delivery = 1;
    let mut proof = PhysicalTextProof::new_without_submit("h").unwrap();
    let mut handoff = KeyboardFocusHandoffState::default();
    let mut routes = XAuthorityClientSurfaceRoutes::default();
    let mut route_batch = super::super::wm_update_coordinator_batch(TransactionId::from_raw(1));
    route_batch.client = Some(sophia_x_authority::XServerFrontendClientId::from_raw(1));
    route_batch
        .surface_routes
        .push(sophia_x_authority::XAuthoritySurfaceRouteObservation {
            surface,
            client: sophia_x_authority::XServerFrontendClientId::from_raw(1),
            admission: None,
        });
    route_batch.transactions.push(SurfaceTransaction {
        input_region: None,
        transaction: TransactionId::from_raw(1),
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: Size {
            width: (geometry).width,
            height: (geometry).height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 1 },
            sophia_protocol::Size {
                width: geometry.width,
                height: geometry.height,
            },
        ),

        damage: Region::single(geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 1_000,
        previous_committed_generation: 0,
    });
    routes.observe(&route_batch).unwrap();

    let held = route_input_events(
        events,
        &focus,
        &committed,
        &[],
        &routes,
        &input_sender,
        &mut modifiers,
        &mut key_repeat,
        &key_repeat_map,
        &mut client_keys,
        &mut emergency,
        &mut virtual_terminal,
        &mut keyboard_coverage,
        None,
        &mut pointer,
        false,
        false,
        false,
        PhysicalInputRoutingMode::ControlPlaneOnly,
        &mut next_delivery,
        10,
        Some(&mut proof),
        Some(&mut handoff),
        None,
    )
    .unwrap();

    assert_eq!(held.keys_routed, 0);
    assert_eq!(held.deferred_key_presses, [(1, 1)]);
    assert!(!proof.is_complete());
    assert_eq!(handoff.target(), Some(surface));
    assert!(input_receiver.try_recv().is_err());

    let released = route_input_events(
        vec![],
        &focus,
        &committed,
        &[],
        &routes,
        &input_sender,
        &mut modifiers,
        &mut key_repeat,
        &key_repeat_map,
        &mut client_keys,
        &mut emergency,
        &mut virtual_terminal,
        &mut keyboard_coverage,
        None,
        &mut pointer,
        false,
        false,
        false,
        PhysicalInputRoutingMode::Full,
        &mut next_delivery,
        11,
        Some(&mut proof),
        Some(&mut handoff),
        Some(surface),
    )
    .unwrap();

    assert_eq!(released.keys_routed, 2);
    assert_eq!(released.keyboard_focus_handoff_released, Some((surface, 2)));
    assert!(proof.is_complete());
    assert_eq!(handoff.target(), None);
    assert_eq!(input_receiver.try_iter().count(), 2);
    assert_eq!(client_keys.pending_len(), 0);
}

#[test]
fn full_routing_suppresses_pointer_buttons_when_workspace_has_no_target() {
    let events = [true, false]
        .into_iter()
        .enumerate()
        .map(|(index, pressed)| InputEventPacket {
            serial: u64::try_from(index + 1).unwrap(),
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(2),
            time_msec: u64::try_from(index + 1).unwrap(),
            kind: InputEventKind::PointerButton {
                button: 0x110,
                pressed,
            },
            global_position: Some(Point { x: 64.0, y: 64.0 }),
            target_surface: None,
            local_position: None,
        })
        .collect();
    let (input_sender, input_receiver) = sync_channel(2);
    let mut modifiers = XCoreKeyboardMapper::new();
    let (mut key_repeat, key_repeat_map) = super::test_key_repeat_parts();
    let mut client_keys = SessionClientKeyState::default();
    let mut emergency = super::super::EmergencyChordState::awaiting_arm();
    let mut virtual_terminal = crate::session_keyboard::VirtualTerminalChordState::default();
    let mut keyboard_coverage = PhysicalKeyboardCoverage::default();
    let mut pointer = SessionPointerPlacement::default();
    pointer.center_on_primary_output(Size {
        width: 2560,
        height: 1440,
    });
    let mut next_delivery = 1;

    let report = route_input_events(
        events,
        &InputFocusState::new(),
        &[],
        &[],
        &XAuthorityClientSurfaceRoutes::default(),
        &input_sender,
        &mut modifiers,
        &mut key_repeat,
        &key_repeat_map,
        &mut client_keys,
        &mut emergency,
        &mut virtual_terminal,
        &mut keyboard_coverage,
        None,
        &mut pointer,
        true,
        false,
        false,
        PhysicalInputRoutingMode::Full,
        &mut next_delivery,
        0,
        None,
        None,
        None,
    )
    .unwrap();

    assert_eq!(report.pointer_buttons_observed, 2);
    assert_eq!(report.pointer_buttons_suppressed_no_target, 2);
    assert_eq!(report.pointer_buttons_suppressed_by_policy, 0);
    assert_eq!(report.pointer_buttons_routed, 0);
    assert!(report.pointer_focus_targets.is_empty());
    assert!(report.deliveries.is_empty());
    assert!(input_receiver.try_recv().is_err());
}

#[test]
fn routed_keyboard_report_retains_the_opaque_focus_target() {
    let seat = SeatId::from_raw(1);
    let surface = SurfaceId::new(41, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 480,
    };
    let committed = [CommittedSurfaceState {
        surface,
        committed_generation: 1,
        geometry,
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 1 },
            sophia_protocol::Size {
                width: geometry.width,
                height: geometry.height,
            },
        ),
        damage: Region::single(geometry),
    }];
    let mut focus = InputFocusState::new();
    assert_eq!(
        focus.focus_surface(seat, surface, &committed),
        InputFocusDecision::Focused
    );
    let events = vec![InputEventPacket {
        serial: 1,
        seat,
        device: DeviceId::from_raw(1),
        time_msec: 1,
        kind: InputEventKind::Key {
            keycode: 30,
            pressed: true,
        },
        global_position: None,
        target_surface: None,
        local_position: None,
    }];
    let (input_sender, input_receiver) = sync_channel(1);
    let mut modifiers = XCoreKeyboardMapper::new();
    let (mut key_repeat, key_repeat_map) = super::test_key_repeat_parts();
    let mut client_keys = SessionClientKeyState::default();
    let mut emergency = super::super::EmergencyChordState::awaiting_arm();
    let mut virtual_terminal = crate::session_keyboard::VirtualTerminalChordState::default();
    let mut keyboard_coverage = PhysicalKeyboardCoverage::default();
    let mut pointer = SessionPointerPlacement::default();
    let mut next_delivery = 1;

    let report = route_input_events(
        events,
        &focus,
        &committed,
        &[],
        &XAuthorityClientSurfaceRoutes::default(),
        &input_sender,
        &mut modifiers,
        &mut key_repeat,
        &key_repeat_map,
        &mut client_keys,
        &mut emergency,
        &mut virtual_terminal,
        &mut keyboard_coverage,
        None,
        &mut pointer,
        false,
        false,
        false,
        PhysicalInputRoutingMode::Full,
        &mut next_delivery,
        0,
        None,
        None,
        None,
    )
    .unwrap();

    assert_eq!(report.keys_routed, 1);
    assert_eq!(report.key_targets, [surface]);
    assert_eq!(report.routed_key_presses, [(1, 1)]);
    assert_eq!(
        input_receiver.try_recv().unwrap().request.target_surface,
        surface
    );
}

#[test]
fn stable_focused_gpu_frame_proves_post_input_pixels() {
    let input_surface = SurfaceId::new(41, 1);
    assert!(stable_gpu_frame_proves_post_input_pixels(
        true,
        Some(input_surface),
        input_surface,
        true,
    ));
    assert!(!stable_gpu_frame_proves_post_input_pixels(
        false,
        Some(input_surface),
        input_surface,
        true,
    ));
    assert!(!stable_gpu_frame_proves_post_input_pixels(
        true,
        Some(input_surface),
        SurfaceId::new(42, 1),
        true,
    ));
    assert!(!stable_gpu_frame_proves_post_input_pixels(
        true,
        Some(input_surface),
        input_surface,
        false,
    ));
}

#[test]
fn physical_input_page_flip_requires_a_changed_post_ingress_submission() {
    assert!(physical_input_page_flip_correlates(
        true, true, 10_000, 4, 5, 11, 13, 11_000, 16_000,
    ));
    assert!(!physical_input_page_flip_correlates(
        false, true, 10_000, 4, 5, 11, 13, 11_000, 16_000,
    ));
    assert!(!physical_input_page_flip_correlates(
        true, false, 10_000, 4, 5, 11, 13, 11_000, 16_000,
    ));
    assert!(!physical_input_page_flip_correlates(
        true, true, 10_000, 5, 5, 11, 13, 11_000, 16_000,
    ));
    assert!(!physical_input_page_flip_correlates(
        true, true, 10_000, 4, 5, 11, 13, 9_999, 16_000,
    ));
    assert!(!physical_input_page_flip_correlates(
        true, true, 10_000, 4, 5, 11, 13, 11_000, 10_999,
    ));
    // A later submission carrying a composition built before the input is
    // the shape every session of the first full physical run reported as a
    // measurement. The flip is real; the picture is older than the press.
    assert!(!physical_input_page_flip_correlates(
        true, true, 10_000, 4, 5, 11, 11, 11_000, 16_000,
    ));
}

#[test]
fn the_newest_head_composition_spans_every_pipeline_stage() {
    // Rendering one frame, holding another submitted, displaying a third:
    // input has to beat all of them, so the baseline is the maximum.
    assert_eq!(
        newest_head_composition_frame([Some(7), Some(11), Some(9), Some(5)]),
        11
    );
    assert_eq!(newest_head_composition_frame([None, Some(3), None]), 3);
    assert_eq!(newest_head_composition_frame([None, None]), 0);
    assert_eq!(newest_head_composition_frame([]), 0);
}

fn pending_delivery_fixture(
    delivery: XAuthorityInputDeliveryId,
) -> crate::input_delivery::PendingInputDelivery {
    crate::input_delivery::PendingInputDelivery {
        ticket: sophia_x_authority::XAuthorityInputDeliveryTicket {
            delivery,
            surface: sophia_protocol::SurfaceId::new(1, 1),
            seat: sophia_protocol::SeatId::from_raw(1),
            control_epoch: 1,
            admitted_at: std::time::Instant::now(),
            client: None,
        },
        release_barrier: true,
    }
}

/// A held grab follows the pointer into the grabbing client's own popup.
///
/// A toolkit grabs on the window that was clicked and only then creates and
/// maps its menu, so the lease anchors to a surface the pointer immediately
/// leaves. Routing to the anchor sent every click inside an open Thunar
/// dropdown to the window beneath it: the menu never received the press that
/// dismisses it and stayed mapped over whatever came next, while the popup was
/// in the projection, under the pointer and ranked above the anchor the whole
/// time.
///
/// The anchor still stands wherever owner_events does not apply -- another
/// client's surface, or no surface at all, which is the drag that leaves the
/// grabbing client's geometry and must keep its ordering.
#[test]
fn a_held_grab_routes_into_the_grabbing_clients_popup_and_nowhere_else() {
    let anchor = SurfaceId::new(4_194_310, 1);
    let popup = SurfaceId::new(4_195_333, 1);
    let stranger = SurfaceId::new(2_097_166, 1);
    let grabbing = sophia_protocol::ClientAdmissionId::from_raw(2);
    let other = sophia_protocol::ClientAdmissionId::from_raw(1);
    let admission_of = |surface: SurfaceId| match surface {
        s if s == anchor || s == popup => Some(grabbing),
        s if s == stranger => Some(other),
        _ => None,
    };

    assert_eq!(
        super::super::grab_routed_surface(Some(popup), anchor, grabbing, admission_of),
        popup,
        "a click inside the grabbing client's own popup belongs to the popup"
    );
    assert_eq!(
        super::super::grab_routed_surface(Some(stranger), anchor, grabbing, admission_of),
        anchor,
        "another client's surface is not the grab's to route to"
    );
    assert_eq!(
        super::super::grab_routed_surface(None, anchor, grabbing, admission_of),
        anchor,
        "a drag outside every surface keeps the anchor, which is grab ordering"
    );
    assert_eq!(
        super::super::grab_routed_surface(Some(anchor), anchor, grabbing, admission_of),
        anchor,
        "the anchor under the pointer is still the anchor"
    );
}
