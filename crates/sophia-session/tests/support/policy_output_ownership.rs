use super::*;
use crate::live_session::ReconciledPublicPolicyProposal;

#[test]
fn policy_placement_assigns_existing_layers_and_reassigns_them_on_output_moves() {
    let first = OutputId::from_raw(1);
    let second = OutputId::from_raw(2);
    let surface = SurfaceId::new(93, 1);
    for (previous, destination) in [(None, first), (Some(first), second)] {
        let mut layout = PersistentLiveLayout::default();
        let mut layer = test_layer(
            surface,
            Rect {
                x: 0,
                y: 0,
                width: 640,
                height: 480,
            },
        );
        // Startup has already observed a raster before the first WM placement.
        // A later move clones the same cached layer with its former output.
        layer.output = previous;
        layout.layers.insert(surface, layer);
        let projection = policy_projection(destination, surface);
        let content = ReconciledPublicPolicyProposal {
            content: BTreeMap::from([(surface, projection.placements[0].clone())]),
            adjusted_surfaces: 0,
            policy: sophia_protocol::PolicyProjectionProposal {
                presentation: None,
                transaction: TransactionId::from_raw(930),
                connection_epoch: 1,
                request_id: 1,
                base_generation: 1,
                active_output: destination,
                outputs: vec![projection.clone()],
                launch_contexts: vec![],
                output_launch_contexts: vec![],
                translation_groups: vec![],
                tab_groups: vec![],
                indicators: vec![],
                output_statuses: vec![],
            },
        };
        let transaction = TransactionId::from_raw(930);
        let proposal = public_live_proposal(
            &layout,
            destination,
            vec![projection],
            transaction,
            LiveWmProposalSource::Manage(surface),
            LivePolicySettlementIdentity {
                connection_epoch: 1,
                request_id: 1,
                scene_generation: 1,
                transaction,
                expect_session_operation: false,
                session_operation: false,
            },
            &content,
        )
        .unwrap();
        let placed = proposal
            .layers
            .iter()
            .find(|layer| layer.surface == surface)
            .unwrap();
        assert_eq!(placed.output, Some(destination));
        assert_eq!(
            layout.layers[&surface].output, previous,
            "a proposal cannot mutate committed placement"
        );
        let owners = BTreeMap::from([(surface, placed.output.unwrap())]);
        assert_eq!(
            sophia_backend_live::live_surfaces_owned_by_output(&[surface], &owners, destination),
            vec![surface]
        );
        let other = if destination == first { second } else { first };
        assert!(
            sophia_backend_live::live_surfaces_owned_by_output(&[surface], &owners, other)
                .is_empty()
        );
    }
}

#[test]
fn partial_drag_projection_preserves_the_other_outputs_committed_content() {
    use sophia_protocol::{
        LayoutNodeCapabilities, PolicyInteractionAxis, PolicyInteractionKind,
        PolicyInteractionPhase, PolicyPresentationState, PolicyProjectionProposal,
        PolicyRequestCause, PolicySurfaceKind, PolicySurfaceSnapshot,
    };
    for active in [1, 2] {
        for kind in [PolicyInteractionKind::Move, PolicyInteractionKind::Resize] {
            for phase in [
                PolicyInteractionPhase::Begin,
                PolicyInteractionPhase::Update,
                PolicyInteractionPhase::End,
                PolicyInteractionPhase::Cancel,
            ] {
                let first = sophia_engine::HeadlessOutput::deterministic();
                let second = sophia_engine::HeadlessOutput {
                    id: OutputId::from_raw(2),
                    ..first
                };
                let chrome = sophia_engine::SurfaceChromeStyle::default();
                let mut scene = crate::live_session::LivePublicPolicyState::initial_scene(
                    &[first, second],
                    first.id,
                    vec![],
                );
                for (index, output) in scene.outputs.iter_mut().enumerate() {
                    output.bounds = Rect {
                        x: index as i32 * 800,
                        y: 0,
                        width: 800,
                        height: 600,
                    };
                    output.work_area = output.bounds;
                }
                let mut layout = PersistentLiveLayout::default();
                for (index, output) in [first.id, second.id].into_iter().enumerate() {
                    let surface = SurfaceId::new(93 + index as u32, 1);
                    let outer = Rect {
                        x: 30 + index as i32 * 800,
                        y: 50,
                        width: 640,
                        height: 480,
                    };
                    scene.surfaces.push(PolicySurfaceSnapshot {
                        surface,
                        generation: 1,
                        current_output: Some(output),
                        kind: PolicySurfaceKind::Toplevel,
                        capabilities: LayoutNodeCapabilities::STANDARD_TOPLEVEL,
                        constraints: SurfaceConstraints {
                            min_size: None,
                            max_size: None,
                        },
                        exact_size: None,
                        requested_state: PolicyPresentationState::default(),
                        current_state: PolicyPresentationState::default(),
                        transient_owner: None,
                        geometry: outer,
                    });
                    let mut layer = test_layer(
                        surface,
                        sophia_engine::content_surface_geometry(outer, chrome).unwrap(),
                    );
                    layer.output = Some(output);
                    layer.stack_rank = index as u32;
                    layer.translation = Some(sophia_protocol::LayerTranslation {
                        connection_epoch: 1,
                        group: index as u64 + 1,
                        x: -40,
                        y: 0,
                    });
                    layout.layers.insert(surface, layer);
                }
                let active_output = OutputId::from_raw(active);
                scene.active_output = active_output;
                let untouched =
                    layout.layers[&SurfaceId::new(if active == 1 { 94 } else { 93 }, 1)].clone();
                let mut reducer = sophia_engine::PolicyProjectionReducer::new(scene).unwrap();
                reducer.connect(1).unwrap();
                let target = SurfaceId::new(if active == 1 { 93 } else { 94 }, 1);
                let geometry = Rect {
                    x: 60 + (active - 1) as i32 * 800,
                    y: 70,
                    width: 640,
                    height: 480,
                };
                let request = reducer
                    .issue_request_with_cause(
                        vec![active_output],
                        PolicyRequestCause::Interaction {
                            phase,
                            kind,
                            axis: PolicyInteractionAxis::None,
                            target,
                            geometry,
                        },
                    )
                    .unwrap();
                let mut output = reducer
                    .committed()
                    .into_iter()
                    .find(|p| p.output == active_output)
                    .unwrap();
                output.placements[0].geometry = geometry;
                output.focus = Some(target);
                let projection = PolicyProjectionProposal {
                    presentation: None,
                    transaction: TransactionId::from_raw(930),
                    connection_epoch: request.connection_epoch,
                    request_id: request.request_id,
                    base_generation: request.scene_generation,
                    active_output,
                    outputs: vec![output],
                    launch_contexts: vec![],
                    output_launch_contexts: vec![],
                    translation_groups: vec![],
                    tab_groups: vec![],
                    indicators: vec![],
                    output_statuses: vec![],
                };
                let bounds = reducer
                    .scene()
                    .outputs
                    .iter()
                    .map(|o| (o.output, o.bounds))
                    .collect();
                let mut reconciled = reconcile_public_policy_proposal(
                    &layout,
                    &projection,
                    &bounds,
                    &bounds,
                    chrome,
                )
                .unwrap();
                let staged = reducer.stage_proposal(&reconciled.policy).unwrap();
                assert_eq!(staged.projections().len(), 2);
                assert_eq!(reconciled.content.len(), 1);
                let materialize =
                    |layout: &PersistentLiveLayout,
                     reconciliation: &ReconciledPublicPolicyProposal| {
                        public_live_proposal(
                            layout,
                            active_output,
                            staged.projections(),
                            projection.transaction,
                            LiveWmProposalSource::PointerGesture {
                                surface: target,
                                mode: if kind == PolicyInteractionKind::Move {
                                    sophia_protocol::WmPointerGestureMode::Move
                                } else {
                                    sophia_protocol::WmPointerGestureMode::Resize
                                },
                            },
                            LivePolicySettlementIdentity {
                                connection_epoch: request.connection_epoch,
                                request_id: request.request_id,
                                scene_generation: request.scene_generation,
                                transaction: projection.transaction,
                                expect_session_operation: false,
                                session_operation: false,
                            },
                            reconciliation,
                        )
                    };
                let proposal = materialize(&layout, &reconciled)
                    .expect("a one-output drag must retain the populated other output");
                assert_eq!(
                    proposal
                        .layers
                        .iter()
                        .find(|l| l.surface == untouched.surface),
                    Some(&untouched)
                );
                assert_eq!(
                    proposal
                        .layers
                        .iter()
                        .find(|l| l.surface == target)
                        .unwrap()
                        .geometry,
                    sophia_engine::content_surface_geometry(geometry, chrome).unwrap()
                );
                assert!(!proposal.requested_sizes.contains_key(&untouched.surface));
                assert_eq!(layout.layers[&untouched.surface], untouched);
                assert_eq!(proposal.focus, Some(target));
                let changed_content = reconciled.content.remove(&target).unwrap();
                assert_eq!(
                    materialize(&layout, &reconciled).err().unwrap().to_string(),
                    "public WM projection has no reconciled content placement",
                    "an updated output must never fall back to old content"
                );
                reconciled.content.insert(target, changed_content);
                let retained_layer = layout.layers.remove(&untouched.surface).unwrap();
                assert_eq!(
                    materialize(&layout, &reconciled).err().unwrap().to_string(),
                    "public WM retained output has no committed content placement",
                    "an untouched output must never lose a visible surface silently"
                );
                layout.layers.insert(untouched.surface, retained_layer);
                assert_eq!(
                    reducer.commit_staged(staged),
                    sophia_protocol::PolicyProjectionOutcome::Committed
                );
            }
        }
    }
}
