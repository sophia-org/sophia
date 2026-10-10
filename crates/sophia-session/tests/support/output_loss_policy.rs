use super::*;
use sophia_protocol::{
    LayoutNodeCapabilities, LayoutNodeKind, OutputId, PolicyOutputProjection,
    PolicyPresentationState, PolicyProjectionOutcome, PolicyProjectionProposal, PolicySurfaceKind,
    PolicySurfacePlacement, PolicySurfaceSnapshot, PolicyTransform, SurfaceConstraints,
    SurfacePlacementPreference, SurfacePresentationRole,
};

// Supplied committed policy and authority facts; no card, WM process or pixel
// retirement. The topology update, scene validation and outgoing cycle are real.
fn occupied_outputs() -> (
    ReloadFixture,
    PersistentLiveLayout,
    [sophia_engine::HeadlessOutput; 2],
) {
    let mut fixture = ReloadFixture::new();
    let left = sophia_engine::HeadlessOutput::deterministic();
    let right = sophia_engine::HeadlessOutput {
        id: OutputId::from_raw(2),
        ..left
    };
    let outputs = [left, right];
    let mut layout = PersistentLiveLayout::default();
    fixture
        .wm
        .update_output_work_areas(&layout, &outputs, left)
        .unwrap();
    let public = fixture.wm.public.as_mut().unwrap();
    public.queue.clear();
    let mut scene = public.reducer.scene().clone();
    scene.generation += 1;
    for (index, output) in scene.outputs.iter_mut().enumerate() {
        let surface = SurfaceId::new(91 + index as u32, 1);
        let geometry = Rect {
            width: 100,
            height: 100,
            ..output.bounds
        };
        let constraints = SurfaceConstraints {
            min_size: None,
            max_size: None,
        };
        let client = sophia_x_authority::XServerFrontendClientId::from_raw(index as u64 + 1);
        let mut batch = wm_update_coordinator_batch(TransactionId::from_raw(index as u64 + 1));
        batch.client = Some(client);
        batch
            .surface_routes
            .push(sophia_x_authority::XAuthoritySurfaceRouteObservation {
                surface,
                client,
                admission: None,
            });
        batch.surface_presentations.push(
            sophia_x_authority::XAuthoritySurfacePresentationObservation {
                surface,
                role: SurfacePresentationRole::PolicyManaged,
                kind: LayoutNodeKind::Toplevel,
                placement_preference: SurfacePlacementPreference::Default,
                owner: None,
                stack_rank: index as u32,
                mapped: true,
                geometry,
                constraints,
                generation: 1,
            },
        );
        assert!(!layout.observe_authority_batch(&batch).client_route_invalid);
        scene.surfaces.push(PolicySurfaceSnapshot {
            surface,
            generation: 1,
            current_output: Some(output.output),
            kind: PolicySurfaceKind::Toplevel,
            capabilities: LayoutNodeCapabilities::STANDARD_TOPLEVEL,
            constraints,
            exact_size: None,
            requested_state: PolicyPresentationState::default(),
            current_state: PolicyPresentationState::default(),
            transient_owner: None,
            geometry,
        });
        output.focus = Some(surface);
    }
    public.reducer = sophia_engine::PolicyProjectionReducer::new(scene).unwrap();
    public.reducer.connect(1).unwrap();
    (fixture, layout, outputs)
}

#[test]
fn removing_an_occupied_output_keeps_its_window_for_a_fresh_policy_cycle() {
    for removed_was_active in [false, true] {
        let (mut fixture, mut layout, [left, right]) = occupied_outputs();
        let (worker, commands, _events) =
            policy_transport_worker::worker_capture::capturing_worker();
        let public = fixture.wm.public.as_mut().unwrap();
        public.worker = Some(worker);
        public.active_output = if removed_was_active {
            right.id
        } else {
            left.id
        };
        let before = public.reducer.scene().clone();
        // The common realization path uses the same scene publisher as this
        // ordinary row helper. The old snapshot still contains right's window.
        assert_eq!(
            fixture
                .wm
                .update_output_work_areas(&layout, &[left], left)
                .expect("output loss must not publish a window bound to a removed output"),
            LiveWmRequestAdmission::Admitted
        );
        let public = fixture.wm.public.as_ref().unwrap();
        let scene = public.reducer.scene();
        assert!(scene.generation > before.generation);
        assert_eq!(scene.active_output, left.id);
        assert_eq!(scene.outputs.len(), 1);
        assert_eq!(scene.outputs[0].focus, Some(before.surfaces[0].surface));
        assert_eq!(
            scene.surfaces.len(),
            2,
            "output loss is not surface withdrawal"
        );
        assert_eq!(scene.surfaces[0].current_output, Some(left.id));
        assert_eq!(
            scene.surfaces[1].current_output, None,
            "policy chooses the replacement placement"
        );
        assert_eq!(scene.surfaces[1].geometry, before.surfaces[1].geometry);
        assert_eq!(scene.surfaces[1].generation, before.surfaces[1].generation);
        assert_eq!(public.reducer.committed().len(), 1);

        assert!(
            fixture
                .wm
                .poll_request(&mut layout, left, true)
                .unwrap()
                .is_none()
        );
        let policy_transport_worker::PolicyTransportCommand::Cycle { request, .. } = commands
            .try_recv()
            .expect("topology loss must issue a fresh cycle")
        else {
            panic!("expected cycle");
        };
        assert_eq!(request.affected_outputs, vec![left.id]);
        let public = fixture.wm.public.as_mut().unwrap();
        assert_eq!(request.scene_generation, public.reducer.scene().generation);
        let proposal = PolicyProjectionProposal {
            transaction: TransactionId::from_raw(70),
            connection_epoch: request.connection_epoch,
            request_id: request.request_id,
            base_generation: request.scene_generation,
            active_output: left.id,
            outputs: vec![PolicyOutputProjection {
                output: left.id,
                placements: before
                    .surfaces
                    .iter()
                    .enumerate()
                    .map(|(i, surface)| PolicySurfacePlacement {
                        surface: surface.surface,
                        surface_generation: surface.generation,
                        geometry: Rect {
                            x: (i * 120) as i32,
                            y: 0,
                            width: 100,
                            height: 100,
                        },
                        requested_size: None,
                        crop: None,
                        transform: PolicyTransform::Identity,
                        presentation: surface.current_state,
                    })
                    .collect(),
                focus: Some(before.surfaces[1].surface),
            }],
            presentation: None,
            launch_contexts: vec![],
            output_launch_contexts: vec![],
            translation_groups: vec![],
            tab_groups: vec![],
            indicators: vec![],
            output_statuses: vec![],
        };
        assert_eq!(
            public.reducer.apply_proposal(&proposal),
            PolicyProjectionOutcome::Committed
        );
        assert!(
            public
                .reducer
                .scene()
                .surfaces
                .iter()
                .all(|s| s.current_output == Some(left.id))
        );
    }
}

#[test]
fn an_unchanged_topology_keeps_both_committed_window_assignments() {
    let (fixture, layout, [left, right]) = occupied_outputs();
    let public = fixture.wm.public.as_ref().unwrap();
    let snapshot = public
        .snapshot(&layout, fixture.wm.candidate_chrome_style())
        .unwrap();
    assert_eq!(
        snapshot
            .surfaces
            .iter()
            .map(|s| s.current_output)
            .collect::<Vec<_>>(),
        vec![Some(left.id), Some(right.id)]
    );
    assert_eq!(
        snapshot.outputs.iter().map(|o| o.focus).collect::<Vec<_>>(),
        snapshot
            .surfaces
            .iter()
            .map(|s| Some(s.surface))
            .collect::<Vec<_>>()
    );
}

#[test]
fn output_return_does_not_restore_a_stale_placement_or_focus() {
    let (mut fixture, layout, [left, right]) = occupied_outputs();
    let public = fixture.wm.public.as_mut().unwrap();
    let request = public
        .reducer
        .issue_request(vec![left.id, right.id])
        .unwrap();
    let proposal = PolicyProjectionProposal {
        transaction: TransactionId::from_raw(71),
        connection_epoch: request.connection_epoch,
        request_id: request.request_id,
        base_generation: request.scene_generation,
        active_output: right.id,
        outputs: public.reducer.committed(),
        presentation: None,
        launch_contexts: vec![],
        output_launch_contexts: vec![],
        translation_groups: vec![],
        tab_groups: vec![],
        indicators: vec![],
        output_statuses: vec![],
    };
    let staged = public.reducer.stage_proposal(&proposal).unwrap();
    fixture
        .wm
        .update_output_work_areas(&layout, &[left], left)
        .unwrap();
    assert_eq!(
        fixture
            .wm
            .public
            .as_mut()
            .unwrap()
            .reducer
            .commit_staged(staged),
        PolicyProjectionOutcome::RejectedStale
    );
    fixture
        .wm
        .update_output_work_areas(&layout, &[left, right], left)
        .unwrap();
    let scene = fixture.wm.public.as_ref().unwrap().reducer.scene();
    assert_eq!(scene.active_output, left.id);
    assert_eq!(scene.outputs[1].generation, 2);
    assert_eq!(scene.outputs[1].focus, None);
    assert_eq!(scene.surfaces[1].current_output, None);
    assert_eq!(
        scene.surfaces.len(),
        2,
        "the next policy chooses the returned placement"
    );
}
