#[test]
fn explicit_pointer_grab_control_activates_and_releases_a_presented_root_anchor() {
    let seat = SeatId::from_raw(1);
    let surface = SurfaceId::new(71, 2);
    let admission = sophia_protocol::ClientAdmissionContext::new(
        sophia_protocol::ClientAdmissionId::from_raw(8),
        sophia_protocol::NamespaceContext::new(
            sophia_protocol::NamespaceId::from_raw(4),
            NamespaceProfile::Confined,
            NamespaceCapabilities::NONE,
        )
        .unwrap(),
        sophia_protocol::ClientAuthProvenance::new(
            sophia_protocol::ClientAuthenticationMethod::PeerCredentials,
            9,
        )
        .unwrap(),
    )
    .unwrap();
    let mut routes = XAuthorityClientSurfaceRoutes::default();
    let mut batch = super::super::wm_update_coordinator_batch(TransactionId::from_raw(1));
    batch.client = Some(sophia_x_authority::XServerFrontendClientId::from_raw(3));
    batch.admission = Some(admission);
    batch
        .surface_routes
        .push(sophia_x_authority::XAuthoritySurfaceRouteObservation {
            surface,
            client: sophia_x_authority::XServerFrontendClientId::from_raw(3),
            admission: Some(admission),
        });
    batch
        .presentation_intents
        .push(sophia_protocol::SurfacePresentationIntent {
            surface,
            kind: sophia_protocol::SurfacePresentationIntentKind::Request,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            presentation_owner: None,
            stack_rank: 0,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 80,
            },
            constraints: sophia_protocol::SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        });
    routes.observe(&batch).unwrap();
    // The control path now reads route and mapped state from the layout rather
    // than from a bare route table, so the surface has to be described as
    // mapped, not merely routed: an unmapped anchor is refused by design.
    batch.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            owner: None,
            stack_rank: 0,
            mapped: true,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 80,
            },
            constraints: sophia_protocol::SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    );
    let mut layout = PersistentLiveLayout::default();
    layout.observe_authority_batch(&batch);
    // The lease state is created before the first request so its control epoch
    // can be quoted by the Prepare; a Prepare naming a different epoch is stale
    // by definition and would be refused.
    let mut leases = ApplicationRouteLeaseState::default();
    let control_epoch = leases.control_epoch();
    let mut pending_grabs = ExplicitPointerGrabQueue::default();
    let mut held_input = PendingLeaseInput::default();
    let (release_sender, _release_receiver) = std::sync::mpsc::sync_channel(8);
    let (client, owner) = sophia_x_authority::x_authority_explicit_pointer_grab_bridge(
        std::num::NonZeroUsize::new(4).unwrap(),
    );
    let prepare_client = client.clone();
    let prepare = std::thread::spawn(move || {
        prepare_client.request(
            admission,
            sophia_x_authority::XAuthorityExplicitPointerGrabRequestKind::Prepare {
                anchor: sophia_x_authority::XAuthorityExplicitPointerGrabAnchor::AdmissionDefault,
                replaces: None,
                // No observation prerequisite: this request does not depend on
                // anything the frontend published after it.
                after_observation: None,
                control_epoch,
            },
        )
    });
    while owner.pending() == 0 {
        std::thread::yield_now();
    }
    let report = loop {
        let report = drain_explicit_pointer_grab_controls(
            &owner,
            &mut leases,
            &mut pending_grabs,
            &layout,
            &mut held_input,
            &release_sender,
            false,
            &InputFocusState::new(),
            seat,
            10,
        )
        .unwrap();
        if report.prepared != 0 {
            break report;
        }
        std::thread::yield_now();
    };
    assert_eq!(report.prepared, 1);
    let sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Prepared(identity) =
        prepare.join().unwrap().unwrap()
    else {
        panic!("root grab was not prepared");
    };
    assert_eq!(leases.lease(seat).unwrap().target_surface, surface);

    let activate_client = client.clone();
    let activate = std::thread::spawn(move || {
        activate_client.request(
            admission,
            sophia_x_authority::XAuthorityExplicitPointerGrabRequestKind::Activate { identity },
        )
    });
    while owner.pending() == 0 {
        std::thread::yield_now();
    }
    let activation_report = loop {
        let report = drain_explicit_pointer_grab_controls(
            &owner,
            &mut leases,
            &mut pending_grabs,
            &layout,
            &mut held_input,
            &release_sender,
            false,
            &InputFocusState::new(),
            seat,
            11,
        )
        .unwrap();
        if report.activated != 0 {
            break report;
        }
        std::thread::yield_now();
    };
    assert_eq!(activation_report.activated, 1);
    assert_eq!(
        activate.join().unwrap().unwrap(),
        sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Activated
    );
    assert_eq!(
        leases.lease(seat).unwrap().phase,
        ApplicationRouteLeasePhase::Active
    );

    let release_client = client.clone();
    let release = std::thread::spawn(move || {
        release_client.request(
            admission,
            sophia_x_authority::XAuthorityExplicitPointerGrabRequestKind::BeginRelease { identity },
        )
    });
    while owner.pending() == 0 {
        std::thread::yield_now();
    }
    loop {
        drain_explicit_pointer_grab_controls(
            &owner,
            &mut leases,
            &mut pending_grabs,
            &layout,
            &mut held_input,
            &release_sender,
            false,
            &InputFocusState::new(),
            seat,
            12,
        )
        .unwrap();
        if leases.lease(seat).is_some_and(|lease| {
            matches!(lease.phase, ApplicationRouteLeasePhase::Releasing { .. })
        }) {
            break;
        }
        std::thread::yield_now();
    }
    assert_eq!(
        release.join().unwrap().unwrap(),
        sophia_x_authority::XAuthorityExplicitPointerGrabResponse::ReleaseReady
    );
    assert!(matches!(
        leases.lease(seat).unwrap().phase,
        ApplicationRouteLeasePhase::Releasing { .. }
    ));

    let finish = std::thread::spawn(move || {
        client.request(
            admission,
            sophia_x_authority::XAuthorityExplicitPointerGrabRequestKind::FinishRelease {
                identity,
            },
        )
    });
    while owner.pending() == 0 {
        std::thread::yield_now();
    }
    let release_report = loop {
        let report = drain_explicit_pointer_grab_controls(
            &owner,
            &mut leases,
            &mut pending_grabs,
            &layout,
            &mut held_input,
            &release_sender,
            false,
            &InputFocusState::new(),
            seat,
            13,
        )
        .unwrap();
        if report.released != 0 {
            break report;
        }
        std::thread::yield_now();
    };
    assert_eq!(release_report.released, 1);
    assert_eq!(
        finish.join().unwrap().unwrap(),
        sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Released
    );
    assert_eq!(leases.lease(seat), None);
}

#[test]
fn floating_pointer_gesture_is_captured_until_one_atomic_completion() {
    let surface = SurfaceId::new(41, 1);
    let start = sophia_protocol::WmPointerPosition { x: 120, y: 80 };
    let end = sophia_protocol::WmPointerPosition { x: 440, y: 300 };
    let initial_geometry = Rect {
        x: 100,
        y: 60,
        width: 300,
        height: 200,
    };
    let mut state = FloatingPointerGestureState::default();

    let ignored = observe_floating_pointer_gesture(
        &mut state,
        InputEventKind::PointerButton {
            button: 0x110,
            pressed: true,
        },
        Some(start),
        Some(surface),
        Some(sophia_protocol::SurfacePresentationRole::PolicyManaged),
        Some(initial_geometry),
        false,
    );
    assert!(!ignored.consumed);

    let press = observe_floating_pointer_gesture(
        &mut state,
        InputEventKind::PointerButton {
            button: 0x111,
            pressed: true,
        },
        Some(start),
        Some(surface),
        Some(sophia_protocol::SurfacePresentationRole::PolicyManaged),
        Some(initial_geometry),
        true,
    );
    assert!(press.consumed);
    assert!(press.completed.is_none());
    assert_eq!(
        press.interaction,
        Some(FloatingPointerPolicyInteraction {
            surface,
            mode: sophia_protocol::WmPointerGestureMode::Resize,
            phase: sophia_protocol::PolicyInteractionPhase::Begin,
            start,
            current: start,
            geometry: initial_geometry,
        })
    );
    assert_eq!(
        press.outline,
        FloatingPointerOutlineUpdate::Set(FloatingPointerOutline {
            surface,
            start,
            geometry: initial_geometry,
        })
    );

    let motion = observe_floating_pointer_gesture(
        &mut state,
        InputEventKind::PointerMotion,
        Some(end),
        Some(surface),
        Some(sophia_protocol::SurfacePresentationRole::PolicyManaged),
        Some(initial_geometry),
        false,
    );
    assert!(motion.consumed);
    assert!(motion.completed.is_none());
    assert_eq!(
        motion.interaction.map(|interaction| interaction.phase),
        Some(sophia_protocol::PolicyInteractionPhase::Update)
    );
    assert_eq!(
        motion.outline,
        FloatingPointerOutlineUpdate::Set(FloatingPointerOutline {
            surface,
            start,
            geometry: Rect {
                x: 100,
                y: 60,
                width: 620,
                height: 420,
            },
        })
    );

    let release = observe_floating_pointer_gesture(
        &mut state,
        InputEventKind::PointerButton {
            button: 0x111,
            pressed: false,
        },
        Some(end),
        None,
        None,
        None,
        false,
    );
    assert!(release.consumed);
    assert_eq!(release.outline, FloatingPointerOutlineUpdate::Clear);
    assert_eq!(
        release.interaction.map(|interaction| interaction.phase),
        Some(sophia_protocol::PolicyInteractionPhase::End)
    );
    assert_eq!(
        release.completed,
        Some(sophia_protocol::WmPointerGestureCompleted {
            surface,
            output: OutputId::INVALID,
            workspace: sophia_protocol::WorkspaceId::INVALID,
            mode: sophia_protocol::WmPointerGestureMode::Resize,
            start,
            end,
        })
    );

    let ordinary_motion = observe_floating_pointer_gesture(
        &mut state,
        InputEventKind::PointerMotion,
        Some(end),
        None,
        None,
        None,
        false,
    );
    assert!(!ordinary_motion.consumed);
    assert!(ordinary_motion.completed.is_none());
}

#[test]
fn floating_pointer_security_cancel_uses_the_latest_reduced_geometry() {
    let surface = SurfaceId::new(42, 1);
    let start = sophia_protocol::WmPointerPosition { x: 100, y: 100 };
    let current = sophia_protocol::WmPointerPosition { x: 180, y: 140 };
    let initial = Rect {
        x: 20,
        y: 30,
        width: 400,
        height: 300,
    };
    let mut state = FloatingPointerGestureState::default();
    let begin = observe_floating_pointer_gesture(
        &mut state,
        InputEventKind::PointerButton {
            button: 0x110,
            pressed: true,
        },
        Some(start),
        Some(surface),
        Some(sophia_protocol::SurfacePresentationRole::PolicyManaged),
        Some(initial),
        true,
    );
    assert!(begin.interaction.is_some());
    let update = observe_floating_pointer_gesture(
        &mut state,
        InputEventKind::PointerMotion,
        Some(current),
        None,
        None,
        None,
        false,
    );
    assert!(update.interaction.is_some());

    assert_eq!(
        state.cancel(),
        Some(FloatingPointerPolicyInteraction {
            surface,
            mode: sophia_protocol::WmPointerGestureMode::Move,
            phase: sophia_protocol::PolicyInteractionPhase::Cancel,
            start,
            current,
            geometry: Rect {
                x: 100,
                y: 70,
                ..initial
            },
        })
    );
    assert!(state.cancel().is_none());
}

#[test]
fn floating_outline_stays_wholly_inside_the_gesture_start_output() {
    let output_one = OutputId::from_raw(1);
    let output_two = OutputId::from_raw(2);
    let outline = FloatingPointerOutline {
        surface: SurfaceId::new(42, 1),
        start: sophia_protocol::WmPointerPosition { x: 1300, y: 200 },
        geometry: Rect {
            x: 700,
            y: 500,
            width: 1400,
            height: 900,
        },
    };

    let clamped = clamp_floating_pointer_outline(
        outline,
        &[
            (
                output_one,
                Rect {
                    x: 0,
                    y: 0,
                    width: 1200,
                    height: 800,
                },
            ),
            (
                output_two,
                Rect {
                    x: 1200,
                    y: 0,
                    width: 800,
                    height: 600,
                },
            ),
        ],
    )
    .unwrap();

    assert_eq!(
        clamped.geometry,
        Rect {
            x: 1200,
            y: 0,
            width: 800,
            height: 600,
        }
    );
}
