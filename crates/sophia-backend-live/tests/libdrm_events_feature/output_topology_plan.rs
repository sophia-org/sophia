fn topology_plan_selection(
    raw: u32,
    size: sophia_protocol::Size,
) -> sophia_backend_live::LibdrmNativePrimaryPlaneSelection {
    sophia_backend_live::LibdrmNativePrimaryPlaneSelection::new(
        drm::control::from_u32(raw).unwrap(),
        drm::control::from_u32(raw + 100).unwrap(),
        drm::control::from_u32(raw + 200).unwrap(),
        size,
        None,
    )
}

fn topology_plan_selection_with_cursor(
    raw: u32,
    size: sophia_protocol::Size,
) -> sophia_backend_live::LibdrmNativePrimaryPlaneSelection {
    topology_plan_selection(raw, size).with_cursor_plane(drm::control::from_u32(raw + 300).unwrap())
}

fn topology_plan_mode() -> drm::control::Mode {
    drm::control::Mode::from(drm_ffi::drm_mode_modeinfo::default())
}

#[test]
fn live_native_topology_plan_resolves_every_enabled_and_disabled_head_without_mutation() {
    use sophia_backend_live::{
        LiveOutputAuthorityDisabledHead, LiveOutputAuthorityHeadTarget,
        LiveProductionNativeTopologyCurrentHead, LiveProductionNativeTopologyDisposition,
        NativeMirrorGrouping,
    };
    use sophia_protocol::{OutputHeadMapping, OutputId, OutputTransform, OutputVrrPolicy, Size};

    let output_one = OutputId::from_raw(1);
    let output_two = OutputId::from_raw(2);
    let head_one = RenderHeadId::from_raw(11);
    let head_two = RenderHeadId::from_raw(12);
    let large = Size {
        width: 2_560,
        height: 1_440,
    };
    let small = Size {
        width: 1_920,
        height: 1_080,
    };
    let current = [
        LiveProductionNativeTopologyCurrentHead::new(
            head_one,
            0,
            output_one,
            topology_plan_selection_with_cursor(1, large),
            4,
        ),
        LiveProductionNativeTopologyCurrentHead::new(
            head_two,
            1,
            output_one,
            topology_plan_selection(2, small),
            7,
        ),
    ];
    let resolved = sophia_backend_live::LiveResolvedOutputTopology {
        primary_output: output_one,
        primary_heads: [(output_one, head_one)].into_iter().collect(),
        outputs: vec![sophia_engine::HeadlessOutput {
            id: output_one,
            size: large,
            scale: 1,
        }],
        logical_viewports: vec![sophia_backend_live::LiveOutputAuthorityLogicalViewport {
            output: output_one,
            logical: Rect {
                x: 0,
                y: 0,
                width: large.width,
                height: large.height,
            },
        }],
        disabled_heads: vec![LiveOutputAuthorityDisabledHead {
            head: head_two,
            target_generation: 8,
        }],
        targets: vec![LiveOutputAuthorityHeadTarget {
            head: head_one,
            target_generation: 5,
            output: output_one,
            timing: sophia_backend_live::LibdrmNativeOutputTiming::new(2_560, 1_440, 60_000),
            native_size: large,
            transform: OutputTransform::Normal,
            mapping: OutputHeadMapping::Exact,
            vrr: OutputVrrPolicy::Disabled,
        }],
        mirror_grouping: NativeMirrorGrouping::none(),
    };
    let mut mode_reads = Vec::new();
    let plan = sophia_backend_live::plan_live_production_native_topology(
        &current,
        &resolved,
        |current, timing| {
            mode_reads.push((current.head, timing));
            Ok(Some(topology_plan_mode()))
        },
    )
    .unwrap();
    assert_eq!(mode_reads.len(), 1, "disabled heads need no mode lookup");
    assert_eq!(plan.primary_output, output_one);
    assert_eq!(plan.heads.len(), 2);
    assert!(matches!(
        plan.heads[0].disposition,
        LiveProductionNativeTopologyDisposition::Enabled { output, selection, .. }
            if output == output_one
                && selection.size() == large
                && selection.cursor_plane() == current[0].selection.cursor_plane()
    ));
    assert_eq!(
        plan.heads[0].previous_selection.cursor_plane(),
        current[0].selection.cursor_plane(),
        "candidate and rollback routes must retain the discovered cursor plane"
    );
    assert_eq!(
        plan.heads[1].disposition,
        LiveProductionNativeTopologyDisposition::Disabled
    );
    assert_eq!(
        current[1].output, output_one,
        "projection must not mutate the published binding"
    );

    let mut split = resolved;
    split.outputs.push(sophia_engine::HeadlessOutput {
        id: output_two,
        size: small,
        scale: 1,
    });
    split
        .logical_viewports
        .push(sophia_backend_live::LiveOutputAuthorityLogicalViewport {
            output: output_two,
            logical: Rect {
                x: large.width,
                y: 0,
                width: small.width,
                height: small.height,
            },
        });
    split.disabled_heads.clear();
    split.primary_heads.insert(output_two, head_two);
    split.targets.push(LiveOutputAuthorityHeadTarget {
        head: head_two,
        target_generation: 8,
        output: output_two,
        timing: sophia_backend_live::LibdrmNativeOutputTiming::new(1_920, 1_080, 60_000),
        native_size: small,
        transform: OutputTransform::Normal,
        mapping: OutputHeadMapping::Exact,
        vrr: OutputVrrPolicy::Disabled,
    });
    let split_plan = sophia_backend_live::plan_live_production_native_topology(
        &current,
        &split,
        |_current, _timing| Ok(Some(topology_plan_mode())),
    )
    .unwrap();
    assert!(matches!(
        split_plan.heads[1].disposition,
        LiveProductionNativeTopologyDisposition::Enabled { output, .. } if output == output_two
    ));

    split.primary_heads.insert(output_two, head_one);
    assert_eq!(
        sophia_backend_live::plan_live_production_native_topology(
            &current,
            &split,
            |_current, _timing| Ok(Some(topology_plan_mode())),
        ),
        Err(
            sophia_backend_live::LiveProductionNativeTopologyPlanError::InvalidPrimaryHead(
                output_two,
            )
        )
    );
}

#[test]
fn live_native_topology_plan_rejects_incomplete_coverage_and_stale_generations() {
    use sophia_backend_live::{
        LiveOutputAuthorityHeadTarget, LiveProductionNativeTopologyCurrentHead,
        LiveProductionNativeTopologyPlanError, NativeMirrorGrouping,
    };
    use sophia_protocol::{OutputHeadMapping, OutputId, OutputTransform, OutputVrrPolicy, Size};

    let output = OutputId::from_raw(1);
    let size = Size {
        width: 1_280,
        height: 720,
    };
    let head_one = RenderHeadId::from_raw(21);
    let head_two = RenderHeadId::from_raw(22);
    let current = [
        LiveProductionNativeTopologyCurrentHead::new(
            head_one,
            0,
            output,
            topology_plan_selection(3, size),
            1,
        ),
        LiveProductionNativeTopologyCurrentHead::new(
            head_two,
            0,
            output,
            topology_plan_selection(4, size),
            1,
        ),
    ];
    let mut resolved = sophia_backend_live::LiveResolvedOutputTopology {
        primary_output: output,
        primary_heads: [(output, head_one)].into_iter().collect(),
        outputs: vec![sophia_engine::HeadlessOutput {
            id: output,
            size,
            scale: 1,
        }],
        logical_viewports: vec![sophia_backend_live::LiveOutputAuthorityLogicalViewport {
            output,
            logical: Rect {
                x: 0,
                y: 0,
                width: size.width,
                height: size.height,
            },
        }],
        disabled_heads: Vec::new(),
        targets: vec![LiveOutputAuthorityHeadTarget {
            head: head_one,
            target_generation: 2,
            output,
            timing: sophia_backend_live::LibdrmNativeOutputTiming::new(1_280, 720, 60_000),
            native_size: size,
            transform: OutputTransform::Normal,
            mapping: OutputHeadMapping::Exact,
            vrr: OutputVrrPolicy::Disabled,
        }],
        mirror_grouping: NativeMirrorGrouping::none(),
    };
    assert_eq!(
        sophia_backend_live::plan_live_production_native_topology(
            &current,
            &resolved,
            |_current, _timing| Ok(Some(topology_plan_mode())),
        ),
        Err(LiveProductionNativeTopologyPlanError::MissingCandidateHead(
            head_two
        ))
    );

    resolved
        .disabled_heads
        .push(sophia_backend_live::LiveOutputAuthorityDisabledHead {
            head: head_two,
            target_generation: 2,
        });
    resolved.targets[0].target_generation = 1;
    assert_eq!(
        sophia_backend_live::plan_live_production_native_topology(
            &current,
            &resolved,
            |_current, _timing| Ok(Some(topology_plan_mode())),
        ),
        Err(LiveProductionNativeTopologyPlanError::InvalidGeneration(
            head_one
        ))
    );
}

fn topology_apply_plan(
    card_indices: &[usize],
) -> sophia_backend_live::LiveProductionNativeTopologyPlan {
    use sophia_backend_live::{
        LiveOutputAuthorityLogicalViewport, LiveProductionNativeTopologyDisposition,
        LiveProductionNativeTopologyHeadPlan, LiveProductionNativeTopologyPlan,
    };
    use sophia_protocol::{OutputHeadMapping, OutputTransform, OutputVrrPolicy};

    let output = OutputId::from_raw(1);
    let size = Size {
        width: 1_920,
        height: 1_080,
    };
    LiveProductionNativeTopologyPlan {
        primary_output: output,
        primary_heads: [(output, RenderHeadId::from_raw(1))].into_iter().collect(),
        outputs: vec![sophia_engine::HeadlessOutput {
            id: output,
            size,
            scale: 1,
        }],
        logical_viewports: vec![LiveOutputAuthorityLogicalViewport {
            output,
            logical: Rect {
                x: 0,
                y: 0,
                width: size.width,
                height: size.height,
            },
        }],
        heads: card_indices
            .iter()
            .copied()
            .enumerate()
            .map(|(index, card_index)| {
                let head = RenderHeadId::from_raw(index as u64 + 1);
                let previous_selection = topology_plan_selection(index as u32 + 1, size);
                LiveProductionNativeTopologyHeadPlan {
                    head,
                    card_index,
                    previous_output: output,
                    previous_enabled: true,
                    previous_selection,
                    previous_target_generation: 1,
                    previous_scale: 1,
                    previous_refresh_millihz: 60_000,
                    previous_transform: OutputTransform::Normal,
                    previous_mapping: OutputHeadMapping::Exact,
                    previous_vrr: OutputVrrPolicy::Disabled,
                    candidate_target_generation: 2,
                    disposition: LiveProductionNativeTopologyDisposition::Enabled {
                        output,
                        selection: previous_selection,
                        scale: 1,
                        refresh_millihz: 60_000,
                        transform: OutputTransform::Normal,
                        mapping: OutputHeadMapping::Exact,
                        vrr: OutputVrrPolicy::Disabled,
                    },
                }
            })
            .collect(),
    }
}

#[test]
fn topology_apply_coordinator_rolls_back_the_accepted_card_prefix_in_reverse_order() {
    use sophia_backend_live::{
        LiveProductionNativeTopologyApplyCoordinator as Coordinator,
        LiveProductionNativeTopologyApplyPhase as Phase,
        LiveProductionNativeTopologyApplyTransition as Transition, NativeTopologySubmitOutcome,
    };

    let mut coordinator = Coordinator::new(&topology_apply_plan(&[9, 3, 7])).unwrap();
    assert_eq!(coordinator.phase(), Phase::Prepared);
    assert_eq!(coordinator.begin_apply(), Transition::Accepted);

    // Cards are canonicalized independently of head discovery order.
    assert_eq!(coordinator.current_card_index(), Some(3));
    assert!(matches!(
        coordinator.observe_apply(3, NativeTopologySubmitOutcome::Accepted),
        Transition::CardApplied { card_index: 3, .. }
    ));
    assert_eq!(coordinator.current_card_index(), Some(7));
    assert!(matches!(
        coordinator.observe_apply(7, NativeTopologySubmitOutcome::Accepted),
        Transition::CardApplied { card_index: 7, .. }
    ));
    assert_eq!(coordinator.current_card_index(), Some(9));
    assert_eq!(
        coordinator.observe_apply(9, NativeTopologySubmitOutcome::Rejected),
        Transition::RollbackRequired {
            failed_card_index: 9
        }
    );

    assert_eq!(coordinator.phase(), Phase::RollingBack);
    assert_eq!(coordinator.current_card_index(), Some(7));
    assert!(matches!(
        coordinator.observe_rollback(7, NativeTopologySubmitOutcome::Accepted),
        Transition::CardRolledBack { card_index: 7, .. }
    ));
    assert_eq!(coordinator.current_card_index(), Some(3));
    assert!(matches!(
        coordinator.observe_rollback(3, NativeTopologySubmitOutcome::Accepted),
        Transition::RolledBack { card_index: 3, .. }
    ));
    assert_eq!(coordinator.phase(), Phase::RolledBack);
    assert_eq!(coordinator.current_card_index(), None);
}

#[test]
fn topology_apply_coordinator_retries_busy_and_distinguishes_unmutated_failure() {
    use sophia_backend_live::{
        LiveProductionNativeTopologyApplyCoordinator as Coordinator,
        LiveProductionNativeTopologyApplyPhase as Phase,
        LiveProductionNativeTopologyApplyTransition as Transition, NativeTopologySubmitOutcome,
    };

    let mut coordinator = Coordinator::new(&topology_apply_plan(&[4, 4])).unwrap();
    assert_eq!(coordinator.begin_apply(), Transition::Accepted);
    assert_eq!(coordinator.current_heads().len(), 2);
    assert_eq!(
        coordinator.observe_apply(4, NativeTopologySubmitOutcome::Busy),
        Transition::Retry
    );
    assert_eq!(coordinator.current_card_index(), Some(4));
    assert!(matches!(
        coordinator.observe_apply(4, NativeTopologySubmitOutcome::Accepted),
        Transition::Applied { card_index: 4, .. }
    ));
    assert_eq!(coordinator.phase(), Phase::Applied);

    let mut rejected = Coordinator::new(&topology_apply_plan(&[2, 5])).unwrap();
    assert_eq!(rejected.begin_apply(), Transition::Accepted);
    assert_eq!(
        rejected.observe_apply(2, NativeTopologySubmitOutcome::Rejected),
        Transition::FailedWithoutMutation { card_index: 2 }
    );
    assert_eq!(rejected.phase(), Phase::Failed);
    assert_eq!(rejected.current_card_index(), None);
}

#[test]
fn topology_apply_coordinator_can_rollback_after_full_apply_before_publication() {
    use sophia_backend_live::{
        LiveProductionNativeTopologyApplyCoordinator as Coordinator,
        LiveProductionNativeTopologyApplyPhase as Phase,
        LiveProductionNativeTopologyApplyTransition as Transition, NativeTopologySubmitOutcome,
    };

    let mut coordinator = Coordinator::new(&topology_apply_plan(&[2, 5])).unwrap();
    assert_eq!(coordinator.begin_apply(), Transition::Accepted);
    assert!(matches!(
        coordinator.observe_apply(2, NativeTopologySubmitOutcome::Accepted),
        Transition::CardApplied { card_index: 2, .. }
    ));
    assert!(matches!(
        coordinator.observe_apply(5, NativeTopologySubmitOutcome::Accepted),
        Transition::Applied { card_index: 5, .. }
    ));
    assert_eq!(coordinator.phase(), Phase::Applied);
    assert_eq!(
        coordinator.begin_rollback_after_apply(),
        Transition::Accepted
    );
    assert_eq!(coordinator.current_card_index(), Some(5));
    assert!(matches!(
        coordinator.observe_rollback(5, NativeTopologySubmitOutcome::Accepted),
        Transition::CardRolledBack { card_index: 5, .. }
    ));
    assert!(matches!(
        coordinator.observe_rollback(2, NativeTopologySubmitOutcome::Accepted),
        Transition::RolledBack { card_index: 2, .. }
    ));
    assert_eq!(coordinator.phase(), Phase::RolledBack);
}

#[test]
fn topology_apply_coordinator_can_abort_an_accepted_prefix_between_owner_turns() {
    use sophia_backend_live::{
        LiveProductionNativeTopologyApplyCoordinator as Coordinator,
        LiveProductionNativeTopologyApplyPhase as Phase,
        LiveProductionNativeTopologyApplyTransition as Transition, NativeTopologySubmitOutcome,
    };

    let mut coordinator = Coordinator::new(&topology_apply_plan(&[2, 5, 8])).unwrap();
    assert_eq!(coordinator.begin_apply(), Transition::Accepted);
    assert!(matches!(
        coordinator.observe_apply(2, NativeTopologySubmitOutcome::Accepted),
        Transition::CardApplied { card_index: 2, .. }
    ));
    assert_eq!(
        coordinator.begin_rollback_after_partial_apply(),
        Transition::Accepted
    );
    assert_eq!(coordinator.phase(), Phase::RollingBack);
    assert_eq!(coordinator.current_card_index(), Some(2));
    assert!(matches!(
        coordinator.observe_rollback(2, NativeTopologySubmitOutcome::Accepted),
        Transition::RolledBack { card_index: 2, .. }
    ));
    assert_eq!(coordinator.phase(), Phase::RolledBack);
}

#[test]
fn published_topology_projection_uses_published_viewports_and_live_native_targets() {
    use sophia_backend_live::{
        LibdrmNativeOutputTiming, LiveProductionNativeTopologyCurrentHead,
        project_live_production_published_topology,
    };
    use sophia_protocol::{
        DisplayHeadId, DisplayModeId, OutputAuthoritySnapshot, OutputGroupMember,
        OutputHeadDescriptor, OutputHeadMapping, OutputLogicalGroupState, OutputModeDescriptor,
        OutputTransformSet,
    };

    let left = OutputId::from_raw(1);
    let right = OutputId::from_raw(2);
    let left_head = RenderHeadId::from_raw(31);
    let right_head = RenderHeadId::from_raw(32);
    let left_size = Size {
        width: 2_560,
        height: 1_440,
    };
    let right_size = Size {
        width: 1_920,
        height: 1_080,
    };
    let descriptor = |head: RenderHeadId, generation, size: Size| OutputHeadDescriptor {
        head: DisplayHeadId::from_raw(head.raw()),
        generation,
        label: format!("Display {}", head.raw()),
        connected: true,
        enabled: true,
        current_mode: Some(DisplayModeId::from_raw(head.raw() * 10)),
        transforms: OutputTransformSet::ALL,
        vrr_capable: true,
        modes: vec![OutputModeDescriptor {
            mode: DisplayModeId::from_raw(head.raw() * 10),
            pixel_size: size,
            refresh_millihz: 60_000,
            preferred: true,
        }],
    };
    let snapshot = OutputAuthoritySnapshot {
        topology_epoch: 8,
        primary_output: left,
        heads: vec![
            descriptor(left_head, 4, left_size),
            descriptor(right_head, 7, right_size),
        ],
        groups: vec![
            OutputLogicalGroupState {
                output: left,
                generation: 3,
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: 1_280,
                    height: 720,
                },
                members: vec![OutputGroupMember {
                    head: DisplayHeadId::from_raw(left_head.raw()),
                    mapping: OutputHeadMapping::Exact,
                }],
            },
            OutputLogicalGroupState {
                output: right,
                generation: 5,
                logical: Rect {
                    x: 1_280,
                    y: 0,
                    width: 1_920,
                    height: 1_080,
                },
                members: vec![OutputGroupMember {
                    head: DisplayHeadId::from_raw(right_head.raw()),
                    mapping: OutputHeadMapping::Fit,
                }],
            },
        ],
    };
    let current = [
        LiveProductionNativeTopologyCurrentHead::new_with_target(
            left_head,
            true,
            0,
            left,
            topology_plan_selection(10, left_size),
            4,
            1,
            60_000,
            sophia_protocol::OutputTransform::Normal,
            OutputHeadMapping::Exact,
            sophia_protocol::OutputVrrPolicy::Disabled,
        ),
        LiveProductionNativeTopologyCurrentHead::new_with_target(
            right_head,
            true,
            1,
            right,
            topology_plan_selection(20, right_size),
            7,
            1,
            60_000,
            sophia_protocol::OutputTransform::Rotate90,
            OutputHeadMapping::Fit,
            sophia_protocol::OutputVrrPolicy::Always,
        ),
    ];

    let projected = project_live_production_published_topology(&current, &snapshot, |head| {
        Ok(LibdrmNativeOutputTiming::new(
            head.selection.size().width as u32,
            head.selection.size().height as u32,
            60_000,
        ))
    })
    .unwrap();

    assert_eq!(
        projected.logical_viewports[0].logical,
        snapshot.groups[0].logical
    );
    assert_eq!(
        projected.logical_viewports[1].logical,
        snapshot.groups[1].logical
    );
    assert_eq!(projected.targets[0].native_size, left_size);
    assert_eq!(projected.targets[0].target_generation, 4);
    assert_eq!(projected.targets[1].native_size, right_size);
    assert_eq!(projected.targets[1].target_generation, 7);
    assert_eq!(projected.targets[1].mapping, OutputHeadMapping::Fit);
    assert_eq!(
        projected.targets[1].transform,
        sophia_protocol::OutputTransform::Rotate90
    );
    assert_eq!(
        projected.targets[1].vrr,
        sophia_protocol::OutputVrrPolicy::Always
    );
}

#[test]
fn topology_resource_cohort_requires_complete_candidate_and_rollback_owners() {
    use sophia_backend_live::{
        LiveProductionNativeTopologyCandidateResource as Candidate,
        LiveProductionNativeTopologyDisposition as Disposition,
        LiveProductionNativeTopologyResourceCohort as Cohort,
        LiveProductionNativeTopologyResourceTransition as Transition,
    };

    let mut plan = topology_apply_plan(&[0, 1]);
    let enabled = plan.heads[0].head;
    let disabled = plan.heads[1].head;
    plan.heads[1].disposition = Disposition::Disabled;
    let mut resources = Cohort::<String, String>::new(&plan).unwrap();

    assert_eq!(
        resources
            .prepare_candidate_enabled(enabled, "candidate-enabled".into())
            .unwrap(),
        Transition::Accepted
    );
    assert_eq!(
        resources
            .prepare_candidate_disabled(disabled, "candidate-disabled".into())
            .unwrap(),
        Transition::Accepted
    );
    assert!(
        !resources.ready(),
        "candidate coverage alone must not permit apply"
    );
    assert_eq!(
        resources
            .prepare_rollback(enabled, "rollback-enabled".into())
            .unwrap(),
        Transition::Accepted
    );
    assert!(!resources.ready());
    assert_eq!(
        resources
            .prepare_rollback(disabled, "rollback-disabled".into())
            .unwrap(),
        Transition::Ready
    );
    assert!(resources.ready());
    assert_eq!(resources.card_heads(0), vec![enabled]);
    assert_eq!(resources.card_heads(1), vec![disabled]);
    assert!(matches!(
        resources.candidate(enabled),
        Some(Candidate::Enabled(_))
    ));
    assert!(matches!(
        resources.candidate(disabled),
        Some(Candidate::Disabled(_))
    ));

    let rejection = resources
        .prepare_candidate_enabled(enabled, "duplicate".into())
        .unwrap_err();
    assert_eq!(rejection.transition, Transition::Duplicate);
    assert_eq!(rejection.owner, "duplicate");
    let rejection = resources
        .prepare_candidate_enabled(disabled, "wrong-role".into())
        .unwrap_err();
    assert_eq!(rejection.transition, Transition::WrongDisposition);
    assert_eq!(rejection.owner, "wrong-role");

    let (candidate, rollback) = resources.into_remaining();
    assert_eq!(candidate.len(), 2);
    assert_eq!(rollback.len(), 2);
}

#[test]
fn topology_resource_cohort_restores_a_previously_disabled_head_without_a_framebuffer() {
    use sophia_backend_live::{
        LiveProductionNativeTopologyCandidateResource as Candidate,
        LiveProductionNativeTopologyResourceCohort as Cohort,
        LiveProductionNativeTopologyResourceTransition as Transition,
    };

    let mut plan = topology_apply_plan(&[0]);
    let head = plan.heads[0].head;
    plan.heads[0].previous_enabled = false;
    let mut resources = Cohort::<String, String>::new(&plan).unwrap();

    assert_eq!(
        resources
            .prepare_candidate_enabled(head, "candidate-enabled".into())
            .unwrap(),
        Transition::Accepted
    );
    let rejection = resources
        .prepare_rollback_enabled(head, "wrong-rollback-owner".into())
        .unwrap_err();
    assert_eq!(rejection.transition, Transition::WrongDisposition);
    assert_eq!(rejection.owner, "wrong-rollback-owner");
    assert_eq!(
        resources
            .prepare_rollback_disabled(head, "rollback-disabled".into())
            .unwrap(),
        Transition::Ready
    );
    assert!(matches!(
        resources.rollback(head),
        Some(Candidate::Disabled(_))
    ));
}

#[test]
fn published_topology_projection_preserves_a_disabled_connected_head() {
    use sophia_backend_live::{
        LiveProductionNativeTopologyCurrentHead, project_live_production_published_topology,
    };
    use sophia_protocol::{
        DisplayHeadId, DisplayModeId, OutputAuthoritySnapshot, OutputGroupMember,
        OutputHeadDescriptor, OutputHeadMapping, OutputLogicalGroupState, OutputModeDescriptor,
        OutputTransformSet,
    };

    let output = OutputId::from_raw(1);
    let enabled = RenderHeadId::from_raw(41);
    let disabled = RenderHeadId::from_raw(42);
    let size = Size {
        width: 1_920,
        height: 1_080,
    };
    let mode = DisplayModeId::from_raw(410);
    let snapshot = OutputAuthoritySnapshot {
        topology_epoch: 9,
        primary_output: output,
        heads: vec![
            OutputHeadDescriptor {
                head: DisplayHeadId::from_raw(enabled.raw()),
                generation: 3,
                label: "enabled".into(),
                connected: true,
                enabled: true,
                current_mode: Some(mode),
                transforms: OutputTransformSet::NORMAL,
                vrr_capable: false,
                modes: vec![OutputModeDescriptor {
                    mode,
                    pixel_size: size,
                    refresh_millihz: 60_000,
                    preferred: true,
                }],
            },
            OutputHeadDescriptor {
                head: DisplayHeadId::from_raw(disabled.raw()),
                generation: 7,
                label: "disabled".into(),
                connected: true,
                enabled: false,
                current_mode: None,
                transforms: OutputTransformSet::NORMAL,
                vrr_capable: false,
                modes: vec![OutputModeDescriptor {
                    mode: DisplayModeId::from_raw(420),
                    pixel_size: size,
                    refresh_millihz: 60_000,
                    preferred: true,
                }],
            },
        ],
        groups: vec![OutputLogicalGroupState {
            output,
            generation: 2,
            logical: Rect {
                x: 0,
                y: 0,
                width: size.width,
                height: size.height,
            },
            members: vec![OutputGroupMember {
                head: DisplayHeadId::from_raw(enabled.raw()),
                mapping: OutputHeadMapping::Exact,
            }],
        }],
    };
    let current = [
        LiveProductionNativeTopologyCurrentHead::new_with_target(
            enabled,
            true,
            0,
            output,
            topology_plan_selection(31, size),
            3,
            1,
            60_000,
            sophia_protocol::OutputTransform::Normal,
            OutputHeadMapping::Exact,
            sophia_protocol::OutputVrrPolicy::Disabled,
        ),
        LiveProductionNativeTopologyCurrentHead::new_with_enabled(
            disabled,
            false,
            0,
            output,
            topology_plan_selection(32, size),
            7,
        ),
    ];

    let projected = project_live_production_published_topology(&current, &snapshot, |head| {
        Ok(sophia_backend_live::LibdrmNativeOutputTiming::new(
            head.selection.size().width as u32,
            head.selection.size().height as u32,
            60_000,
        ))
    })
    .unwrap();

    assert_eq!(projected.targets.len(), 1);
    assert_eq!(projected.targets[0].head, enabled);
    assert_eq!(
        projected.disabled_heads,
        vec![sophia_backend_live::LiveOutputAuthorityDisabledHead {
            head: disabled,
            target_generation: 7,
        }]
    );
}

fn topology_composition_frame(
    plan: &sophia_backend_live::LiveProductionNativeTopologyPlan,
    head: RenderHeadId,
    candidate: bool,
    size_override: Option<Size>,
) -> sophia_backend_live::LiveProductionHeadCompositionFrame {
    use sophia_backend_live::LiveProductionNativeTopologyDisposition as Disposition;

    let head_plan = plan
        .heads
        .iter()
        .find(|head_plan| head_plan.head == head)
        .expect("test topology head exists");
    let (output, size, scale, target_generation, mapping) = if candidate {
        match head_plan.disposition {
            Disposition::Enabled {
                output,
                selection,
                scale,
                mapping,
                ..
            } => (
                output,
                selection.size(),
                scale,
                head_plan.candidate_target_generation,
                mapping,
            ),
            Disposition::Disabled => panic!("disabled candidate has no composition frame"),
        }
    } else {
        (
            head_plan.previous_output,
            head_plan.previous_selection.size(),
            head_plan.previous_scale,
            head_plan.previous_target_generation,
            head_plan.previous_mapping,
        )
    };
    let size = size_override.unwrap_or(size);
    sophia_backend_live::LiveProductionHeadCompositionFrame {
        head,
        scene_generation: 11,
        target_generation,
        mapping,
        logical_content_checksum: output.raw(),
        frame: sophia_renderer_live::LiveOwnedMixedCompositionFrame {
            image_reads: Default::default(),
            layers: Vec::new(),
            output_damage_snapshot: Some(sophia_engine::OutputFrameDamageSnapshot {
                damage_history: Default::default(),
                output: sophia_engine::HeadlessOutput {
                    id: output,
                    size,
                    scale,
                },
                surfaces: Vec::new(),
                compositor_display_list: sophia_engine::CompositorDisplayList {
                    output,
                    commands: Vec::new(),
                },
                software_cursor: None,
            }),
            trace: None,
            direct_scanout: Default::default(),
        },
    }
}

fn identified_head_composition_frame(
    target: sophia_engine::HeadRenderTarget,
    scene_generation: u64,
    checksum: u64,
) -> sophia_backend_live::LiveProductionHeadCompositionFrame {
    sophia_backend_live::LiveProductionHeadCompositionFrame {
        head: target.head,
        scene_generation,
        target_generation: target.target_generation,
        mapping: target.mapping,
        logical_content_checksum: checksum,
        frame: sophia_renderer_live::LiveOwnedMixedCompositionFrame {
            image_reads: Default::default(),
            layers: Vec::new(),
            output_damage_snapshot: Some(sophia_engine::OutputFrameDamageSnapshot {
                damage_history: Default::default(),
                output: sophia_engine::HeadlessOutput {
                    id: target.output,
                    size: target.native_size,
                    scale: target.scale,
                },
                surfaces: Vec::new(),
                compositor_display_list: sophia_engine::CompositorDisplayList {
                    output: target.output,
                    commands: Vec::new(),
                },
                software_cursor: None,
            }),
            trace: None,
            direct_scanout: Default::default(),
        },
    }
}

#[test]
fn topology_frame_admission_separates_candidate_and_rollback_coverage() {
    use sophia_backend_live::{
        LiveProductionNativeTopologyDisposition as Disposition,
        validate_live_production_topology_frames,
    };

    let mut plan = topology_apply_plan(&[0, 1]);
    let enabled = plan.heads[0].head;
    let disabled = plan.heads[1].head;
    let size = plan.heads[0].previous_selection.size();
    plan.heads[1].disposition = Disposition::Disabled;

    let candidate = validate_live_production_topology_frames(
        &plan,
        vec![topology_composition_frame(&plan, enabled, true, None)],
        true,
    )
    .unwrap();
    assert_eq!(candidate.keys().copied().collect::<Vec<_>>(), vec![enabled]);

    assert!(
        validate_live_production_topology_frames(
            &plan,
            vec![topology_composition_frame(&plan, enabled, false, None)],
            false,
        )
        .is_err(),
        "rollback requires an enabled owner for a candidate-disabled head"
    );
    let rollback = validate_live_production_topology_frames(
        &plan,
        vec![
            topology_composition_frame(&plan, enabled, false, None),
            topology_composition_frame(&plan, disabled, false, None),
        ],
        false,
    )
    .unwrap();
    assert_eq!(rollback.len(), 2);

    assert!(
        validate_live_production_topology_frames(
            &plan,
            vec![topology_composition_frame(
                &plan,
                enabled,
                true,
                Some(Size {
                    width: size.width - 1,
                    height: size.height,
                }),
            )],
            true,
        )
        .is_err(),
        "candidate damage must name its exact native target"
    );
}

#[test]
fn topology_renderer_image_requirements_are_scoped_per_physical_head() {
    use sophia_backend_live::live_topology_frame_renderer_image_requirements;
    use sophia_renderer_live::{
        LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888, LiveCompositionPlacement, LiveRendererImageId,
    };

    let plan = topology_apply_plan(&[0, 1]);
    let first = plan.heads[0].head;
    let second = plan.heads[1].head;
    let mut first_frame = topology_composition_frame(&plan, first, true, None);
    let second_frame = topology_composition_frame(&plan, second, true, None);
    let placement = LiveCompositionPlacement {
        target: sophia_protocol::Rect {
            x: 0,
            y: 0,
            width: 16,
            height: 16,
        },
        clip: None,
        transform: sophia_protocol::Transform::IDENTITY,
        alpha: 1.0,
        sampling: sophia_engine::HeadSamplingClass::Exact,
    };
    for image_id in [2, 1, 2] {
        first_frame.frame.layers.push(
            sophia_renderer_live::LiveOwnedMixedCompositionLayer::RendererImage {
                image_id: LiveRendererImageId::from_raw(image_id),
                size: sophia_protocol::Size {
                    width: 16,
                    height: 16,
                },
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                placement,
            },
        );
    }
    let frames = [(first, first_frame), (second, second_frame)]
        .into_iter()
        .collect();

    let requirements = live_topology_frame_renderer_image_requirements(&frames);

    assert_eq!(
        requirements.get(&first),
        Some(&vec![
            LiveRendererImageId::from_raw(1),
            LiveRendererImageId::from_raw(2),
        ])
    );
    assert!(!requirements.contains_key(&second));
}

#[test]
fn current_heads_reduce_to_independent_committed_render_targets() {
    use sophia_backend_live::{
        LiveProductionNativeTopologyCurrentHead, reduce_live_production_head_render_target,
    };
    use sophia_protocol::{OutputHeadMapping, OutputId, OutputTransform, OutputVrrPolicy, Size};

    let output = OutputId::from_raw(41);
    let large = LiveProductionNativeTopologyCurrentHead::new_with_target(
        RenderHeadId::from_raw(101),
        true,
        0,
        output,
        topology_plan_selection(
            51,
            Size {
                width: 2_560,
                height: 1_440,
            },
        ),
        7,
        1,
        144_000,
        OutputTransform::Normal,
        OutputHeadMapping::Exact,
        OutputVrrPolicy::Always,
    );
    let small = LiveProductionNativeTopologyCurrentHead::new_with_target(
        RenderHeadId::from_raw(102),
        true,
        1,
        output,
        topology_plan_selection(
            52,
            Size {
                width: 1_920,
                height: 1_080,
            },
        ),
        19,
        2,
        60_000,
        OutputTransform::Rotate90,
        OutputHeadMapping::Cover,
        OutputVrrPolicy::Disabled,
    );
    let disabled = LiveProductionNativeTopologyCurrentHead {
        enabled: false,
        ..small
    };

    let large = reduce_live_production_head_render_target(large).unwrap();
    let small = reduce_live_production_head_render_target(small).unwrap();
    assert_eq!(large.target_generation, 7);
    assert_eq!(large.mapping, OutputHeadMapping::Exact);
    assert_eq!(large.refresh_millihz, 144_000);
    assert_eq!(small.target_generation, 19);
    assert_eq!(small.mapping, OutputHeadMapping::Cover);
    assert_eq!(small.scale, 2);
    assert_eq!(small.transform, OutputTransform::Rotate90);
    assert!(reduce_live_production_head_render_target(disabled).is_none());
}

#[test]
fn head_frame_batch_rejects_stale_generation_mapping_and_scene_identity() {
    use sophia_backend_live::validate_live_head_composition_frame_batch;
    use sophia_engine::HeadRenderTarget;
    use sophia_protocol::{OutputHeadMapping, OutputId, OutputTransform, Size};

    let output = OutputId::from_raw(51);
    let large = HeadRenderTarget {
        head: RenderHeadId::from_raw(201),
        output,
        target_generation: 7,
        native_size: Size {
            width: 2_560,
            height: 1_440,
        },
        scale: 1,
        refresh_millihz: 144_000,
        transform: OutputTransform::Normal,
        mapping: OutputHeadMapping::Exact,
    };
    let small = HeadRenderTarget {
        head: RenderHeadId::from_raw(202),
        output,
        target_generation: 19,
        native_size: Size {
            width: 1_920,
            height: 1_080,
        },
        scale: 2,
        refresh_millihz: 60_000,
        transform: OutputTransform::Normal,
        mapping: OutputHeadMapping::Cover,
    };
    let expected = [large, small];
    let frames = [
        identified_head_composition_frame(large, 33, 77),
        identified_head_composition_frame(small, 33, 77),
    ];
    assert_eq!(
        validate_live_head_composition_frame_batch(output, &expected, &frames),
        Ok(77)
    );

    let mut stale = identified_head_composition_frame(small, 33, 77);
    stale.target_generation = 18;
    assert_eq!(
        validate_live_head_composition_frame_batch(
            output,
            &expected,
            &[identified_head_composition_frame(large, 33, 77), stale],
        ),
        Err("head composition targets a stale native generation")
    );
    let mut wrong_mapping = identified_head_composition_frame(small, 33, 77);
    wrong_mapping.mapping = OutputHeadMapping::Fit;
    assert_eq!(
        validate_live_head_composition_frame_batch(
            output,
            &expected,
            &[
                identified_head_composition_frame(large, 33, 77),
                wrong_mapping,
            ],
        ),
        Err("head composition mapping does not match its native target")
    );
    assert_eq!(
        validate_live_head_composition_frame_batch(
            output,
            &expected,
            &[
                identified_head_composition_frame(large, 33, 77),
                identified_head_composition_frame(small, 34, 77),
            ],
        ),
        Err("head composition frames disagree on scene generation")
    );
}

#[test]
fn semantic_startup_requires_every_worker_and_prepared_head_before_kms() {
    use sophia_backend_live::{
        LiveProductionSemanticStartupBarrier as Barrier,
        reduce_live_production_semantic_startup_barrier,
    };
    use std::collections::BTreeSet;

    let head_one = RenderHeadId::from_raw(71);
    let head_two = RenderHeadId::from_raw(72);
    let required = [head_one, head_two];
    let one = BTreeSet::from([head_one]);
    let both = BTreeSet::from([head_one, head_two]);

    assert_eq!(
        reduce_live_production_semantic_startup_barrier(&[head_one], &one, &one),
        Barrier::Ready,
        "single-head outputs use the same semantic startup transaction",
    );
    assert_eq!(
        reduce_live_production_semantic_startup_barrier(
            &required,
            &BTreeSet::new(),
            &BTreeSet::new(),
        ),
        Barrier::Waiting,
    );
    assert_eq!(
        reduce_live_production_semantic_startup_barrier(&required, &both, &one),
        Barrier::Waiting,
    );
    assert_eq!(
        reduce_live_production_semantic_startup_barrier(&required, &both, &both),
        Barrier::Ready,
    );
    assert_eq!(
        reduce_live_production_semantic_startup_barrier(&required, &one, &both),
        Barrier::Invalid,
        "a prepared owner cannot precede its renderer worker",
    );
    assert_eq!(
        reduce_live_production_semantic_startup_barrier(
            &required,
            &BTreeSet::from([head_one, RenderHeadId::from_raw(99)]),
            &one,
        ),
        Barrier::Invalid,
        "a foreign head cannot enter the startup transaction",
    );
}
