use crate::live_session::*;
use sophia_engine::{ApplicationRouteLeaseBinding, ApplicationRouteLeaseReadiness};
use sophia_protocol::{
    ClientAdmissionContext, InputEventKind, InputEventPacket, NamespaceCapabilities,
    NamespaceProfile, OutputId, Point,
};
use sophia_x_authority::{
    XAuthorityExplicitPointerGrabAnchor as Anchor,
    XAuthorityExplicitPointerGrabRequestKind as Control,
    XAuthorityExplicitPointerGrabResponse as Response,
};

fn admission(client: u64, namespace: u64) -> ClientAdmissionContext {
    ClientAdmissionContext::new(
        sophia_protocol::ClientAdmissionId::from_raw(client),
        sophia_protocol::NamespaceContext::new(
            sophia_protocol::NamespaceId::from_raw(namespace),
            NamespaceProfile::Confined,
            NamespaceCapabilities::NONE,
        )
        .unwrap(),
        sophia_protocol::ClientAuthProvenance::new(
            sophia_protocol::ClientAuthenticationMethod::PeerCredentials,
            1,
        )
        .unwrap(),
    )
    .unwrap()
}

fn add_surface(
    layout: &mut PersistentLiveLayout,
    surface: SurfaceId,
    admission: ClientAdmissionContext,
    x: i32,
) -> LayerSnapshot {
    let client = sophia_x_authority::XServerFrontendClientId::from_raw(admission.client_id.raw());
    let geometry = Rect {
        x,
        y: 0,
        width: 100,
        height: 100,
    };
    let mut batch = wm_update_coordinator_batch(TransactionId::from_raw(7));
    batch.client = Some(client);
    batch.admission = Some(admission);
    batch
        .surface_routes
        .push(sophia_x_authority::XAuthoritySurfaceRouteObservation {
            surface,
            client,
            admission: Some(admission),
        });
    batch.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface,
            role: sophia_protocol::SurfacePresentationRole::ClientPositioned,
            kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            owner: None,
            stack_rank: 0,
            mapped: true,
            geometry,
            constraints: sophia_protocol::SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    );
    assert!(!layout.observe_authority_batch(&batch).client_route_invalid);
    LayerSnapshot {
        surface,
        authority_local_id: None,
        namespace: Some(admission.namespace.id),
        stack_rank: surface.index(),
        geometry,
        source_size: Size {
            width: 100,
            height: 100,
        },
        source: BufferSource::CpuBuffer { handle: 1 },
        damage: Region::empty(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
        input_region: None,
        translation: None,
        output: None,
    }
}

fn event(serial: u64, kind: InputEventKind, x: f64) -> InputEventPacket {
    InputEventPacket {
        serial,
        seat: SeatId::from_raw(1),
        device: sophia_protocol::DeviceId::from_raw(1),
        time_msec: serial,
        kind,
        global_position: Some(Point { x, y: 10.0 }),
        target_surface: None,
        local_position: None,
    }
}

fn projection(layers: Vec<LayerSnapshot>) -> sophia_backend_live::LivePresentedInputProjection {
    sophia_backend_live::LivePresentedInputProjection {
        output: OutputId::from_raw(1),
        epoch: 5,
        layers,
        chrome_targets: Vec::new(),
        chrome_occlusion: None,
        descriptor_targets: Vec::new(),
        descriptor_occlusion: None,
        content: None,
        descriptor_projection: None,
        tab_occlusions: Vec::new(),
    }
}

fn bound_lease(
    state: &mut ApplicationRouteLeaseState,
    surface: SurfaceId,
    admission: ClientAdmissionContext,
) -> sophia_engine::ApplicationRouteLease {
    let lease = state
        .begin_provisional(ApplicationRouteLeaseCandidate {
            seat: SeatId::from_raw(1),
            origin: sophia_engine::ApplicationRouteLeaseOrigin::ExplicitPointer,
            target_surface: surface,
            admission: admission.client_id,
            scope: ApplicationRouteScope {
                profile: admission.namespace.profile,
                authority: admission.namespace.id,
            },
            authority_session_epoch: 1,
            binding: ApplicationRouteLeaseBinding::Bound {
                output: OutputId::from_raw(1),
                revision: 1,
            },
            initiating_device: None,
            initiating_button: None,
        })
        .unwrap();
    state
        .confirm(lease.identity, surface, admission.client_id, 1)
        .unwrap()
}

#[test]
fn retained_scope_includes_compositor_occlusion_and_foreign_apps() {
    let mut layout = PersistentLiveLayout::default();
    let surface = SurfaceId::new(101, 1);
    let own = admission(1, 4);
    let first = add_surface(&mut layout, surface, own, 0);
    let same = add_surface(&mut layout, SurfaceId::new(102, 1), admission(2, 4), 100);
    let foreign = add_surface(&mut layout, SurfaceId::new(103, 1), admission(3, 5), 200);
    let mut scene = projection(vec![first, same, foreign]);
    let mut state = ApplicationRouteLeaseState::default();
    let lease = bound_lease(&mut state, surface, own);
    let motion = event(1, InputEventKind::PointerMotion, 110.0);
    let scope = |scene: &sophia_backend_live::LivePresentedInputProjection,
                 event: &InputEventPacket| {
        presented_application_scope(
            event,
            &scene.layers,
            &scene.chrome_targets,
            scene.chrome_occlusion,
            &scene.descriptor_targets,
            scene.descriptor_occlusion,
            &scene.tab_occlusions,
            &layout.client_routes,
        )
    };
    // Another application in the same namespace, outside the original target.
    let same_scope = scope(&scene, &motion).unwrap();
    authorize_presented_lease(
        &mut state,
        lease,
        &motion,
        &layout.client_routes,
        same_scope,
        scene.output,
        9,
        &scene.layers,
    )
    .unwrap();
    assert_eq!(
        state.lease(lease.identity.seat).unwrap().binding(),
        ApplicationRouteLeaseBinding::Bound {
            output: scene.output,
            revision: 9
        }
    );
    let foreign_motion = event(2, InputEventKind::PointerMotion, 210.0);
    assert_eq!(
        authorize_presented_lease(
            &mut state,
            lease,
            &foreign_motion,
            &layout.client_routes,
            scope(&scene, &foreign_motion).unwrap(),
            scene.output,
            9,
            &scene.layers
        ),
        Err(sophia_engine::ApplicationRouteLeaseError::OutsideScope)
    );
    let occlusion = Rect {
        x: 100,
        y: 0,
        width: 100,
        height: 100,
    };
    scene.chrome_occlusion = Some(occlusion);
    assert!(
        scope(&scene, &motion).is_none(),
        "chrome blocks the underlying application"
    );
    scene.chrome_occlusion = None;
    scene.descriptor_occlusion = Some(occlusion);
    assert!(
        scope(&scene, &motion).is_none(),
        "descriptor modal/panel blocks the underlying application"
    );
    scene.descriptor_occlusion = None;
    scene.tab_occlusions.push(occlusion);
    assert_eq!(
        scope(&scene, &motion),
        Some(same_scope),
        "application pixels are composed above tab chrome"
    );
    scene.layers[1].input_region = Some(Region::empty());
    assert!(
        scope(&scene, &motion).is_none(),
        "an exposed tab through an input hole is not application scope"
    );
}

#[test]
fn held_events_keep_button_order_and_revalidate_current_scope() {
    let mut layout = PersistentLiveLayout::default();
    let surface = SurfaceId::new(104, 1);
    let own = admission(1, 4);
    let layer = add_surface(&mut layout, surface, own, 0);
    let mut scene = projection(vec![layer]);
    let mut state = ApplicationRouteLeaseState::default();
    let lease = bound_lease(&mut state, surface, own);
    let mut held = PendingLeaseInput::default();
    for (serial, kind) in [
        (
            1,
            InputEventKind::PointerButton {
                button: 0x110,
                pressed: true,
            },
        ),
        (2, InputEventKind::PointerMotion),
        (3, InputEventKind::PointerMotion),
        (
            4,
            InputEventKind::PointerButton {
                button: 0x110,
                pressed: false,
            },
        ),
    ] {
        held.defer(lease, scene.output, 10, event(serial, kind, 10.0))
            .unwrap();
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(8);
    let (release_sender, _release_receiver) = std::sync::mpsc::sync_channel(8);
    let mut report = PhysicalInputRouteReport::default();
    let mut next = 1;
    flush_held_lease_input(
        &mut held,
        &mut state,
        &layout.client_routes,
        &[scene.clone()],
        &sender,
        &release_sender,
        &mut next,
        11,
        &mut report,
    )
    .unwrap();
    let serials: Vec<_> = receiver
        .try_iter()
        .map(|r: XAuthorityRoutedInput| r.request.serial)
        .collect();
    assert_eq!(serials, [1, 3, 4]);
    assert_eq!(report.pointer_buttons_routed, 2);
    held.defer(
        lease,
        scene.output,
        12,
        event(
            5,
            InputEventKind::PointerButton {
                button: 0x110,
                pressed: true,
            },
            10.0,
        ),
    )
    .unwrap();
    scene.descriptor_occlusion = Some(Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 100,
    });
    flush_held_lease_input(
        &mut held,
        &mut state,
        &layout.client_routes,
        &[scene],
        &sender,
        &release_sender,
        &mut next,
        13,
        &mut report,
    )
    .unwrap();
    assert!(receiver.try_recv().is_err());
    assert_eq!(
        state.routing_readiness(lease.identity.seat),
        Some(ApplicationRouteLeaseReadiness::Releasing)
    );
    assert!(held.expired(u64::MAX).is_none());
}

#[test]
fn held_events_cannot_outlive_the_binding_or_survive_cancellation() {
    let own = admission(1, 4);
    let mut state = ApplicationRouteLeaseState::default();
    let mut candidate = ApplicationRouteLeaseCandidate {
        seat: SeatId::from_raw(1),
        origin: sophia_engine::ApplicationRouteLeaseOrigin::ExplicitPointer,
        target_surface: SurfaceId::new(105, 1),
        admission: own.client_id,
        scope: ApplicationRouteScope {
            profile: own.namespace.profile,
            authority: own.namespace.id,
        },
        authority_session_epoch: 1,
        binding: ApplicationRouteLeaseBinding::AwaitingPresentation {
            pinned_output: None,
            deadline_msec: 4000,
        },
        initiating_device: None,
        initiating_button: None,
    };
    let lease = state.begin_provisional(candidate).unwrap();
    let mut held = PendingLeaseInput::default();
    held.defer(
        lease,
        OutputId::from_raw(1),
        3000,
        event(1, InputEventKind::PointerMotion, 10.0),
    )
    .unwrap();
    assert_eq!(held.expired(3999), None);
    assert_eq!(held.expired(4000), Some(lease.identity));
    held.cancel(lease.identity);
    assert_eq!(held.expired(u64::MAX), None);
    state.reject(lease.identity).unwrap();
    candidate.binding = ApplicationRouteLeaseBinding::Bound {
        output: OutputId::from_raw(1),
        revision: 1,
    };
    let replacement = state.begin_provisional(candidate).unwrap();
    held.defer(
        replacement,
        OutputId::from_raw(1),
        10,
        event(2, InputEventKind::PointerMotion, 10.0),
    )
    .unwrap();
    held.cancel(lease.identity);
    assert_eq!(
        held.expired(4010),
        Some(replacement.identity),
        "late cancellation names only its own identity"
    );
    for serial in 3..=258 {
        let outcome = held.defer(
            replacement,
            OutputId::from_raw(1),
            10,
            event(
                serial,
                InputEventKind::PointerButton {
                    button: 0x110,
                    pressed: serial % 2 == 0,
                },
                10.0,
            ),
        );
        if serial == 258 {
            assert!(outcome.is_err());
        } else {
            assert!(outcome.is_ok());
        }
    }
    assert_eq!(
        held.expired(u64::MAX),
        None,
        "capacity failure drops the whole sequence"
    );
}

struct GrabFixture {
    layout: PersistentLiveLayout,
    state: ApplicationRouteLeaseState,
    pending: ExplicitPointerGrabQueue,
    held: PendingLeaseInput,
    client: sophia_x_authority::XAuthorityExplicitPointerGrabClient,
    owner: sophia_x_authority::XAuthorityExplicitPointerGrabOwner,
    release_sender: SyncSender<XAuthorityRouteLeaseRelease>,
    _releases: Receiver<XAuthorityRouteLeaseRelease>,
    admission: ClientAdmissionContext,
    surface: SurfaceId,
}

impl GrabFixture {
    fn new() -> Self {
        let mut layout = PersistentLiveLayout::default();
        let admission = admission(1, 4);
        let surface = SurfaceId::new(110, 1);
        let _ = add_surface(&mut layout, surface, admission, 0);
        let (client, owner) = sophia_x_authority::x_authority_explicit_pointer_grab_bridge(
            std::num::NonZeroUsize::new(4).unwrap(),
        );
        let (release_sender, releases) = std::sync::mpsc::sync_channel(8);
        Self {
            layout,
            state: ApplicationRouteLeaseState::default(),
            pending: ExplicitPointerGrabQueue::default(),
            held: PendingLeaseInput::default(),
            client,
            owner,
            release_sender,
            _releases: releases,
            admission,
            surface,
        }
    }
    fn prepare(&self, after_observation: Option<TransactionId>) -> Control {
        Control::Prepare {
            anchor: Anchor::Surface(self.surface),
            replaces: None,
            after_observation,
            control_epoch: self.state.control_epoch(),
        }
    }
    fn start(&self, kind: Control) -> std::thread::JoinHandle<Response> {
        let client = self.client.clone();
        let admission = self.admission;
        let thread = std::thread::spawn(move || client.request(admission, kind).unwrap());
        let deadline = Instant::now() + Duration::from_millis(400);
        while self.owner.pending() == 0 {
            assert!(Instant::now() < deadline, "request was not queued");
            std::thread::yield_now();
        }
        thread
    }
    fn service(&mut self) -> ExplicitPointerGrabControlReport {
        drain_explicit_pointer_grab_controls(
            &self.owner,
            &mut self.state,
            &mut self.pending,
            &self.layout,
            &mut self.held,
            &self.release_sender,
            false,
            &InputFocusState::new(),
            SeatId::from_raw(1),
            10,
        )
        .unwrap()
    }
    fn issue(&mut self, kind: Control) -> Response {
        let response = self.start(kind);
        self.service();
        response.join().unwrap()
    }
}

#[test]
fn mapped_popup_can_reserve_before_any_pixels_or_future_client_traffic() {
    let mut fixture = GrabFixture::new();
    assert!(fixture.layout.layers.is_empty(), "the client has not drawn");
    assert!(fixture.layout.input_eligible(fixture.surface));
    let Response::Prepared(identity) = fixture.issue(fixture.prepare(None)) else {
        panic!("mapped popup was refused")
    };
    assert_eq!(
        fixture.issue(Control::Activate { identity }),
        Response::Activated
    );
    assert_eq!(
        fixture.state.routing_readiness(identity.seat),
        Some(ApplicationRouteLeaseReadiness::WaitForPresentation)
    );
}

#[test]
fn prepare_waits_for_accounted_receipt_and_synthetic_ticks_cannot_satisfy_it() {
    let mut fixture = GrabFixture::new();
    let response = fixture.start(fixture.prepare(Some(TransactionId::from_raw(7))));
    assert_eq!(fixture.service().deferred, 1);
    assert!(fixture.state.lease(SeatId::from_raw(1)).is_none());
    fixture
        .pending
        .account(&wm_update_coordinator_batch(TransactionId::from_raw(999)));
    assert_eq!(fixture.service().deferred, 1);
    let mut observed = wm_update_coordinator_batch(TransactionId::from_raw(6));
    observed.client = Some(sophia_x_authority::XServerFrontendClientId::from_raw(1));
    fixture.pending.account(&observed);
    assert_eq!(fixture.service().deferred, 1);
    observed.transaction = TransactionId::from_raw(7);
    fixture.pending.account(&observed);
    assert_eq!(fixture.service().prepared, 1);
    assert!(matches!(response.join().unwrap(), Response::Prepared(_)));
}

#[test]
fn queued_prepare_cannot_cross_a_control_epoch_change() {
    let mut fixture = GrabFixture::new();
    let response = fixture.start(fixture.prepare(Some(TransactionId::from_raw(7))));
    fixture.state.security_transition().unwrap();
    assert_eq!(fixture.service().rejected, 1);
    assert!(matches!(response.join().unwrap(), Response::Rejected(_)));
    let mut observed = wm_update_coordinator_batch(TransactionId::from_raw(7));
    observed.client = Some(sophia_x_authority::XServerFrontendClientId::from_raw(1));
    fixture.pending.account(&observed);
    fixture.service();
    assert!(fixture.state.lease(SeatId::from_raw(1)).is_none());
}

#[test]
fn abort_after_activation_releases_ownership_and_all_held_input() {
    let mut fixture = GrabFixture::new();
    let Response::Prepared(identity) = fixture.issue(fixture.prepare(None)) else {
        panic!("mapped popup was refused")
    };
    assert_eq!(
        fixture.issue(Control::Activate { identity }),
        Response::Activated
    );
    let lease = fixture.state.lease(identity.seat).unwrap();
    fixture
        .held
        .defer(
            lease,
            OutputId::from_raw(1),
            10,
            event(1, InputEventKind::PointerMotion, 10.0),
        )
        .unwrap();
    assert_eq!(
        fixture.issue(Control::Abort { identity }),
        Response::Aborted
    );
    assert_eq!(
        fixture.state.routing_readiness(identity.seat),
        Some(ApplicationRouteLeaseReadiness::Releasing)
    );
    assert!(fixture.held.expired(u64::MAX).is_none());
    assert!(matches!(
        fixture.issue(Control::Activate { identity }),
        Response::Rejected(_)
    ));
    assert_eq!(
        fixture.issue(Control::FinishRelease { identity }),
        Response::Released
    );
    assert!(fixture.state.lease(identity.seat).is_none());
}

#[test]
fn a_bound_grab_routes_physical_motion_and_release_after_a_scene_change() {
    let mut layout = PersistentLiveLayout::default();
    let target = SurfaceId::new(201, 1);
    let own = admission(1, 4);
    let layer = add_surface(&mut layout, target, own, 0);
    let unrelated = add_surface(&mut layout, SurfaceId::new(202, 1), admission(2, 4), 100);
    layout.presentation_roles.insert(
        target,
        sophia_protocol::SurfacePresentationRole::PolicyManaged,
    );
    let scene = projection(vec![layer, unrelated]);
    let mut state = ApplicationRouteLeaseState::default();
    let lease = bound_lease(&mut state, target, own);
    let (sender, receiver) = sync_channel(8);
    let (release_sender, _release_receiver) = sync_channel(8);
    let (mut repeat, keymap) = super::test_key_repeat_parts();
    let mut pointer = SessionPointerPlacement::default();
    pointer.center_on_primary_output(Size {
        width: 100,
        height: 100,
    });
    let mut held = PendingLeaseInput::default();
    let report = route_input_events_with_launcher(
        vec![
            event(1, InputEventKind::PointerMotion, 1.0),
            event(
                2,
                InputEventKind::PointerButton {
                    button: 272,
                    pressed: false,
                },
                0.0,
            ),
        ],
        &InputFocusState::new(),
        &[],
        &scene.layers,
        &layout.presentation_roles,
        &layout.client_routes,
        &sender,
        &mut XCoreKeyboardMapper::new(),
        &mut repeat,
        &keymap,
        &mut SessionClientKeyState::default(),
        &mut EmergencyChordState::awaiting_arm(),
        &mut VirtualTerminalChordState::default(),
        &mut PhysicalKeyboardCoverage::default(),
        None,
        &mut pointer,
        true,
        false,
        false,
        PhysicalInputRoutingMode::Full,
        &mut 1,
        10,
        None,
        None,
        None,
        Some(target),
        None,
        Some(&mut state),
        None,
        None,
        None,
        Some(&release_sender),
        Some(scene.output),
        scene.epoch,
        None,
        None,
        None,
        None,
        Some(&mut held),
    )
    .unwrap();
    assert!(
        report.policy_inputs.is_empty(),
        "a presented managed window under a grab must not request hover focus"
    );
    assert_eq!(report.pointer_lease_rejections, 0);
    assert_eq!(report.pointer_routed, 2);
    assert_eq!(report.pointer_buttons_routed, 1);
    assert_eq!(receiver.try_iter().count(), 2);
    let current = state.lease(lease.identity.seat).unwrap();
    assert_eq!(current.identity, lease.identity);
    assert_eq!(
        current.binding(),
        ApplicationRouteLeaseBinding::Bound {
            output: scene.output,
            revision: scene.epoch,
        }
    );
}

#[test]
fn client_ungrab_joins_an_engine_release_without_extending_it() {
    let mut fixture = GrabFixture::new();
    let Response::Prepared(identity) = fixture.issue(fixture.prepare(None)) else {
        panic!("mapped popup was refused")
    };
    assert_eq!(
        fixture.issue(Control::Activate { identity }),
        Response::Activated
    );
    cancel_application_lease(
        &mut fixture.state,
        &fixture.layout.client_routes,
        &fixture.release_sender,
        &mut fixture.held,
        identity,
        1,
    )
    .unwrap();
    let releasing = fixture.state.lease(identity.seat).unwrap();
    assert_eq!(
        fixture.issue(Control::BeginRelease { identity }),
        Response::ReleaseReady
    );
    assert_eq!(fixture.state.lease(identity.seat), Some(releasing));
    assert_eq!(
        fixture.state.request_exact_release(
            identity,
            sophia_protocol::ClientAdmissionId::from_raw(999),
            20
        ),
        Err(sophia_engine::ApplicationRouteLeaseError::IdentityMismatch)
    );
    assert_eq!(
        fixture.issue(Control::FinishRelease { identity }),
        Response::Released
    );
    assert!(fixture.state.lease(identity.seat).is_none());
}
