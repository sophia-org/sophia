use sophia_engine::PolicyProjectionReducer;
use sophia_protocol::{
    LayoutNodeCapabilities, OutputId, PolicyOutputProjection, PolicyOutputSnapshot,
    PolicyPresentationState, PolicyProjectionIndicator, PolicyProjectionOutcome,
    PolicyProjectionOutputStatus, PolicyProjectionProposal, PolicyRequestCause,
    PolicySceneSnapshot, PolicySurfaceKind, PolicySurfacePlacement, PolicySurfaceSnapshot,
    PolicyTransform, Rect, SurfaceConstraints, SurfaceId, TransactionId, WmActionId,
};

fn instance_presentation() -> sophia_protocol::PolicyPresentation {
    use sophia_protocol::*;
    PolicyPresentation {
        generation: 1,
        keyboard_output: None,
        outputs: vec![PolicyPresentationOutput {
            output: output(1),
            generation: 1,
            coverage: rect(0, 0),
            mode: PolicyPresentationMode::Overlay,
        }],
        instances: vec![PolicySurfaceInstance {
            id: 1,
            generation: 1,
            output: output(1),
            source: surface_id(1),
            destination: rect(0, 0),
            clip: rect(0, 0),
            opacity_millis: 1000,
            z_index: 0,
            action: None,
        }],
        regions: Vec::new(),
        bindings: Vec::new(),
    }
}

#[test]
fn presentation_action_membership_and_local_revocation_reject_late_successors() {
    use sophia_protocol::PolicyPresentationIdentity;
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();
    reducer.connect(1).unwrap();
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let mut initial = proposal(&request, 1, vec![projected(output(1), vec![], None)]);
    let mut p = instance_presentation();
    p.instances[0].action = Some(WmActionId::from_raw(10));
    initial.presentation = Some(p);
    assert_eq!(
        reducer.apply_proposal(&initial),
        PolicyProjectionOutcome::Committed
    );
    let action = PolicyRequestCause::PresentationAction {
        activation_serial: 1,
        action: WmActionId::from_raw(10),
        identity: PolicyPresentationIdentity {
            publication_generation: 1,
            output: output(1),
            output_generation: 1,
            presentation_epoch: 17,
            target_id: 1,
            target_generation: 1,
        },
    };
    for case in 0..6 {
        let PolicyRequestCause::PresentationAction {
            activation_serial,
            mut action,
            mut identity,
        } = action
        else {
            unreachable!()
        };
        match case {
            0 => identity.publication_generation += 1,
            1 => identity.target_generation += 1,
            2 => identity.output_generation += 1,
            3 => identity.target_id += 1,
            4 => action = WmActionId::from_raw(11),
            _ => {
                identity.target_id = 0;
                identity.target_generation = 0;
            }
        }
        assert!(
            reducer
                .issue_request_with_cause(
                    vec![output(1)],
                    PolicyRequestCause::PresentationAction {
                        activation_serial,
                        action,
                        identity
                    }
                )
                .is_err()
        );
    }
    assert!(
        reducer
            .issue_request_with_cause(vec![output(2)], action)
            .is_err()
    );
    let request = reducer
        .issue_request_with_cause(vec![output(1)], action)
        .unwrap();
    let mut reply = proposal(&request, 2, vec![projected(output(1), vec![], None)]);
    reply.presentation = initial.presentation;
    let staged = reducer.stage_proposal(&reply).unwrap();
    reducer.revoke_presentation();
    assert!(!reducer.presentation_cause_is_current(action));
    assert_eq!(
        reducer.revalidate_staged(&staged),
        PolicyProjectionOutcome::RejectedStale
    );
    let mut late = reducer.clone();
    assert_eq!(
        late.apply_proposal(&reply),
        PolicyProjectionOutcome::RejectedStale
    );
    assert_eq!(
        reducer.commit_staged(staged),
        PolicyProjectionOutcome::RejectedStale
    );
    assert!(reducer.presentation_publication().is_none());
}

#[test]
fn presentation_requires_current_sources_output_generations_and_affected_scope() {
    for case in 0..4 {
        let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();
        reducer.connect(1).unwrap();
        let request = reducer.issue_request(vec![output(1)]).unwrap();
        let mut candidate = proposal(&request, 1, vec![projected(output(1), vec![], None)]);
        let mut p = instance_presentation();
        match case {
            0 => p.instances[0].source = surface_id(99),
            1 => p.outputs[0].generation += 1,
            2 => p.outputs[0].coverage.width = i32::MAX,
            _ => {
                p.outputs[0].output = output(2);
                p.instances[0].output = output(2);
                p.outputs[0].coverage.x = 1000;
                p.instances[0].destination.x = 1000;
                p.instances[0].clip.x = 1000;
            }
        }
        candidate.presentation = Some(p);
        assert_eq!(
            reducer.apply_proposal(&candidate),
            PolicyProjectionOutcome::RejectedInvalid,
            "case {case}"
        );
        assert!(reducer.presentation_publication().is_none());
        assert_eq!(reducer.commit_serial(), 0);
    }
}

#[test]
fn presentation_is_staged_atomically_and_content_changes_preserve_target_identity() {
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();
    reducer.connect(1).unwrap();
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let mut proposal = proposal(&request, 1, vec![projected(output(1), vec![], None)]);
    proposal.presentation = Some(instance_presentation());
    let staged = reducer.stage_proposal(&proposal).unwrap();
    assert!(reducer.presentation_publication().is_none());
    assert_eq!(
        reducer.commit_staged(staged),
        PolicyProjectionOutcome::Committed
    );
    let before = reducer.presentation_publication().unwrap().1.clone();
    let mut updated_source = surface(1);
    updated_source.generation += 1;
    reducer.observe_scene(scene(2, &[updated_source])).unwrap();
    assert_eq!(reducer.presentation_publication().unwrap().1, &before);
    assert!(
        reducer.committed()[0].placements.is_empty(),
        "preview-only source must stay unplaced"
    );
}

#[test]
fn target_mutation_and_retired_identity_reuse_cannot_replace_committed_presentation() {
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();
    reducer.connect(1).unwrap();
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let mut initial = proposal(&request, 1, vec![projected(output(1), vec![], None)]);
    initial.presentation = Some(instance_presentation());
    assert_eq!(
        reducer.apply_proposal(&initial),
        PolicyProjectionOutcome::Committed
    );
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let mut changed = proposal(&request, 2, vec![projected(output(1), vec![], None)]);
    let mut p = instance_presentation();
    p.generation = 2;
    p.instances[0].opacity_millis = 500;
    changed.presentation = Some(p);
    assert_eq!(
        reducer.apply_proposal(&changed),
        PolicyProjectionOutcome::RejectedInvalid
    );
    assert_eq!(
        reducer.presentation_publication().unwrap().1,
        initial.presentation.as_ref().unwrap()
    );
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let closed = proposal(&request, 3, vec![projected(output(1), vec![], None)]);
    assert_eq!(
        reducer.apply_proposal(&closed),
        PolicyProjectionOutcome::Committed
    );
    assert!(reducer.presentation_publication().is_none());
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let mut reused = proposal(&request, 4, vec![projected(output(1), vec![], None)]);
    let mut p = instance_presentation();
    p.generation = 3;
    p.instances[0].generation = 3;
    reused.presentation = Some(p);
    assert_eq!(
        reducer.apply_proposal(&reused),
        PolicyProjectionOutcome::RejectedInvalid
    );
    assert!(reducer.presentation_publication().is_none());
}

#[test]
fn source_loss_topology_change_and_reconnect_revoke_presentation() {
    for cause in 0..3 {
        let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();
        reducer.connect(1).unwrap();
        let request = reducer.issue_request(vec![output(1)]).unwrap();
        let mut p = proposal(&request, 1, vec![projected(output(1), vec![], None)]);
        p.presentation = Some(instance_presentation());
        assert_eq!(
            reducer.apply_proposal(&p),
            PolicyProjectionOutcome::Committed
        );
        match cause {
            0 => reducer.observe_scene(scene(2, &[])).unwrap(),
            1 => {
                let mut next = scene(2, &[surface(1)]);
                next.outputs[1].generation += 1;
                reducer.observe_scene(next).unwrap();
            }
            _ => {
                reducer.disconnect(1);
                reducer.connect(2).unwrap();
            }
        }
        assert!(reducer.presentation_publication().is_none());
        assert_ne!(
            reducer.apply_proposal(&p),
            PolicyProjectionOutcome::Committed
        );
    }
}

#[test]
fn translation_groups_cannot_duplicate_members_or_cross_affected_outputs() {
    for invalid_case in 0..4 {
        let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();
        reducer.connect(1).unwrap();
        let request = reducer.issue_request(vec![output(1)]).unwrap();
        let mut proposal = proposal(
            &request,
            7,
            vec![projected(
                output(1),
                vec![placed(surface_id(1), 1, rect(0, 0))],
                Some(surface_id(1)),
            )],
        );
        proposal
            .translation_groups
            .push(sophia_protocol::PolicyTranslationGroup {
                output: output(1),
                group: 1,
                x: -100,
                y: 0,
                members: vec![surface_id(1)],
            });
        match invalid_case {
            0 => proposal.translation_groups[0].members.push(surface_id(1)),
            1 => proposal.translation_groups[0].output = output(2),
            2 => proposal.translation_groups[0].members[0] = surface_id(2),
            _ => proposal.outputs[0].placements[0].presentation.fullscreen = true,
        }
        assert_eq!(
            reducer.apply_proposal(&proposal),
            PolicyProjectionOutcome::RejectedInvalid
        );
        assert_eq!(reducer.commit_serial(), 0);
    }
}

#[test]
fn duplicate_surface_across_outputs_rejects_without_partial_commit() {
    let scene = scene(1, &[surface(1), surface(2)]);
    let mut reducer = PolicyProjectionReducer::new(scene).unwrap();
    reducer.connect(1).unwrap();
    let request = reducer.issue_request(vec![output(1), output(2)]).unwrap();
    let proposal = proposal(
        &request,
        7,
        vec![
            projected(output(1), vec![placed(surface_id(1), 1, rect(0, 0))], None),
            projected(
                output(2),
                vec![placed(surface_id(1), 1, rect(100, 0))],
                None,
            ),
        ],
    );

    assert_eq!(
        reducer.apply_proposal(&proposal),
        PolicyProjectionOutcome::RejectedInvalid
    );
    assert_eq!(reducer.commit_serial(), 0);
    assert!(
        reducer
            .committed()
            .iter()
            .all(|projection| projection.placements.is_empty())
    );
}

#[test]
fn one_proposal_replaces_both_outputs_atomically() {
    let scene = scene(1, &[surface(1), surface(2)]);
    let mut reducer = PolicyProjectionReducer::new(scene).unwrap();
    reducer.connect(1).unwrap();
    let request = reducer.issue_request(vec![output(1), output(2)]).unwrap();
    let proposal = proposal(
        &request,
        8,
        vec![
            projected(
                output(1),
                vec![placed(surface_id(1), 1, rect(0, 0))],
                Some(surface_id(1)),
            ),
            projected(
                output(2),
                vec![placed(surface_id(2), 1, rect(100, 0))],
                Some(surface_id(2)),
            ),
        ],
    );

    assert_eq!(
        reducer.apply_proposal(&proposal),
        PolicyProjectionOutcome::Committed
    );
    assert_eq!(reducer.commit_serial(), 1);
    assert_eq!(
        reducer
            .committed()
            .iter()
            .map(|projection| projection.placements.len())
            .collect::<Vec<_>>(),
        vec![1, 1]
    );
}

#[test]
fn staged_projection_preserves_last_good_until_frontend_commit() {
    let scene = scene(1, &[surface(1)]);
    let mut reducer = PolicyProjectionReducer::new(scene).unwrap();
    reducer.connect(1).unwrap();
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let proposal = proposal(
        &request,
        8,
        vec![projected(
            output(1),
            vec![placed(surface_id(1), 1, rect(0, 0))],
            Some(surface_id(1)),
        )],
    );

    let staged = reducer.stage_proposal(&proposal).unwrap();
    assert_eq!(reducer.commit_serial(), 0);
    assert!(reducer.committed()[0].placements.is_empty());
    assert_eq!(
        reducer.revalidate_staged(&staged),
        PolicyProjectionOutcome::Committed
    );
    assert_eq!(reducer.commit_serial(), 0);
    assert!(reducer.committed()[0].placements.is_empty());
    assert_eq!(
        reducer.commit_staged(staged),
        PolicyProjectionOutcome::Committed
    );
    assert_eq!(reducer.commit_serial(), 1);
    assert_eq!(reducer.committed()[0].placements.len(), 1);
}

#[test]
fn frontend_fact_change_invalidates_a_staged_projection() {
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();
    reducer.connect(1).unwrap();
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let proposal = proposal(
        &request,
        8,
        vec![projected(
            output(1),
            vec![placed(surface_id(1), 1, rect(0, 0))],
            Some(surface_id(1)),
        )],
    );
    let staged = reducer.stage_proposal(&proposal).unwrap();
    reducer.observe_scene(scene(2, &[surface(1)])).unwrap();

    assert_eq!(
        reducer.commit_staged(staged),
        PolicyProjectionOutcome::RejectedStale
    );
    assert_eq!(reducer.commit_serial(), 0);
    assert!(reducer.committed()[0].placements.is_empty());
    assert!(
        reducer.issue_request(vec![output(1)]).is_ok(),
        "the exact stale request must be retired so fresh policy work can proceed"
    );
}

#[test]
fn output_loss_invalidates_a_staged_multi_output_projection() {
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1), surface(2)])).unwrap();
    reducer.connect(1).unwrap();
    let request = reducer.issue_request(vec![output(1), output(2)]).unwrap();
    let proposal = proposal(
        &request,
        9,
        vec![
            projected(
                output(1),
                vec![placed(surface_id(1), 1, rect(0, 0))],
                Some(surface_id(1)),
            ),
            projected(
                output(2),
                vec![placed(surface_id(2), 1, rect(100, 0))],
                Some(surface_id(2)),
            ),
        ],
    );
    let staged = reducer.stage_proposal(&proposal).unwrap();
    let mut after_loss = scene(2, &[surface(1), surface(2)]);
    after_loss
        .outputs
        .retain(|snapshot| snapshot.output == output(1));
    reducer.observe_scene(after_loss).unwrap();

    assert_eq!(
        reducer.revalidate_staged(&staged),
        PolicyProjectionOutcome::RejectedStale
    );
    assert_eq!(
        reducer.commit_staged(staged),
        PolicyProjectionOutcome::RejectedStale
    );
    assert_eq!(reducer.commit_serial(), 0);
    assert_eq!(reducer.committed().len(), 1);
    assert_eq!(reducer.committed()[0].output, output(1));
    assert!(reducer.committed()[0].placements.is_empty());
}

#[test]
fn guessed_future_generation_cannot_become_current() {
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();
    reducer.connect(1).unwrap();
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    reducer.observe_scene(scene(2, &[surface(1)])).unwrap();
    let mut guessed = proposal(
        &request,
        9,
        vec![projected(
            output(1),
            vec![placed(surface_id(1), 1, rect(0, 0))],
            None,
        )],
    );
    guessed.base_generation = 2;

    assert_eq!(
        reducer.apply_proposal(&guessed),
        PolicyProjectionOutcome::RejectedStale
    );
    assert_eq!(reducer.commit_serial(), 0);
}

#[test]
fn disconnect_preserves_layout_and_scene_removal_prunes_only_dead_surface() {
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1), surface(2)])).unwrap();
    reducer.connect(3).unwrap();
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let proposal = proposal(
        &request,
        10,
        vec![projected(
            output(1),
            vec![
                placed(surface_id(1), 1, rect(0, 0)),
                placed(surface_id(2), 1, rect(50, 0)),
            ],
            Some(surface_id(2)),
        )],
    );
    assert_eq!(
        reducer.apply_proposal(&proposal),
        PolicyProjectionOutcome::Committed
    );
    let committed = reducer.committed();
    assert_eq!(reducer.disconnect(3), PolicyProjectionOutcome::Disconnected);
    assert_eq!(reducer.committed(), committed);

    reducer.observe_scene(scene(2, &[surface(1)])).unwrap();
    let first = reducer
        .committed()
        .into_iter()
        .find(|projection| projection.output == output(1))
        .unwrap();
    assert_eq!(first.placements.len(), 1);
    assert_eq!(first.placements[0].surface, surface_id(1));
    assert_eq!(first.focus, None);
}

#[test]
fn descriptors_commit_atomically_and_clear_at_the_connection_boundary() {
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();
    reducer.connect(5).unwrap();
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let mut candidate = proposal(&request, 44, vec![projected(output(1), Vec::new(), None)]);
    candidate.indicators.push(PolicyProjectionIndicator {
        output: output(1),
        slot: 0,
        indicator: 9,
        action: Some(WmActionId::from_raw(11)),
        state_bits: 1,
        label: "1".into(),
    });
    candidate
        .output_statuses
        .push(PolicyProjectionOutputStatus {
            output: output(1),
            focus_bits: 0,
            layout: "Scroller".into(),
        });

    assert_eq!(
        reducer.apply_proposal(&candidate),
        PolicyProjectionOutcome::Committed
    );
    let committed = reducer.indicator_publication();
    assert_eq!(committed.connection_epoch, Some(5));
    assert_eq!(reducer.commit_serial(), 1);
    assert_eq!(committed.indicators[0].label, "1");

    assert_eq!(reducer.disconnect(5), PolicyProjectionOutcome::Disconnected);
    let cleared = reducer.indicator_publication();
    assert_eq!(cleared.connection_epoch, None);
    assert!(cleared.indicators.is_empty());
    assert!(cleared.output_statuses.is_empty());
    assert!(cleared.generation > committed.generation);
    assert_eq!(reducer.commit_serial(), 1);
}

#[test]
fn invalid_descriptor_preserves_the_last_good_projection_and_publication() {
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();
    reducer.connect(1).unwrap();
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let mut invalid = proposal(&request, 45, vec![projected(output(1), Vec::new(), None)]);
    invalid.indicators.push(PolicyProjectionIndicator {
        output: output(1),
        slot: 0,
        indicator: 0,
        action: None,
        state_bits: 0,
        label: "bad".into(),
    });
    let before = reducer.indicator_publication();

    assert_eq!(
        reducer.apply_proposal(&invalid),
        PolicyProjectionOutcome::RejectedInvalid
    );
    assert_eq!(reducer.indicator_publication(), before);
    assert_eq!(reducer.commit_serial(), 0);
}

#[test]
fn snapshot_membership_and_focus_are_the_initial_and_committed_truth() {
    let mut initial = scene(1, &[surface(1)]);
    initial.surfaces[0].current_output = Some(output(1));
    initial.outputs[0].focus = Some(surface_id(1));
    let mut reducer = PolicyProjectionReducer::new(initial).unwrap();
    assert_eq!(reducer.committed()[0].placements.len(), 1);
    assert_eq!(reducer.committed()[0].focus, Some(surface_id(1)));

    reducer.connect(1).unwrap();
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let hide = proposal(&request, 13, vec![projected(output(1), Vec::new(), None)]);
    assert_eq!(
        reducer.apply_proposal(&hide),
        PolicyProjectionOutcome::Committed
    );
    assert_eq!(reducer.scene().surfaces[0].current_output, None);
    assert_eq!(reducer.scene().outputs[0].focus, None);
}

#[test]
fn scene_focus_must_name_a_live_focusable_surface_on_the_same_output() {
    let mut valid_surface = surface(1);
    valid_surface.current_output = Some(output(1));
    let mut valid = scene(1, &[valid_surface]);
    valid.outputs[0].focus = Some(surface_id(1));
    assert!(PolicyProjectionReducer::new(valid.clone()).is_ok());

    let mut dangling = valid.clone();
    dangling.surfaces.clear();
    let mut wrong_output = valid.clone();
    wrong_output.surfaces[0].current_output = Some(output(2));
    let mut unfocusable = valid.clone();
    unfocusable.surfaces[0].capabilities.focusable = false;
    let mut minimized = valid;
    minimized.surfaces[0].current_state.minimized = true;

    for invalid in [dangling, wrong_output, unfocusable, minimized] {
        assert_eq!(
            PolicyProjectionReducer::new(invalid).unwrap_err(),
            sophia_engine::PolicyProjectionError::InvalidFocus
        );
    }
}

#[test]
fn repeated_action_token_with_distinct_activation_serials_is_not_coalesced() {
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();
    reducer.connect(1).unwrap();

    for activation_serial in [41, 42] {
        let request = reducer
            .issue_request_with_cause(
                vec![output(1)],
                PolicyRequestCause::Action {
                    activation_serial,
                    action: WmActionId::from_raw(7),
                },
            )
            .unwrap();
        assert_eq!(
            request.cause,
            PolicyRequestCause::Action {
                activation_serial,
                action: WmActionId::from_raw(7),
            }
        );
        assert_eq!(
            reducer.apply_proposal(&proposal(
                &request,
                activation_serial,
                vec![projected(output(1), Vec::new(), None)],
            )),
            PolicyProjectionOutcome::Committed
        );
    }

    assert_eq!(reducer.commit_serial(), 2);
}

#[test]
fn work_area_must_be_nonempty_and_inside_output_bounds() {
    let mut invalid = scene(1, &[surface(1)]);
    invalid.outputs[0].work_area = Rect {
        x: 0,
        y: 0,
        width: 101,
        height: 100,
    };

    assert!(PolicyProjectionReducer::new(invalid).is_err());
}

#[test]
fn committed_presentation_state_is_reflected_in_the_next_snapshot() {
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();
    reducer.connect(1).unwrap();
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let mut placement = placed(surface_id(1), 1, rect(0, 0));
    placement.presentation.maximized = true;

    assert_eq!(
        reducer.apply_proposal(&proposal(
            &request,
            44,
            vec![projected(output(1), vec![placement], Some(surface_id(1)))],
        )),
        PolicyProjectionOutcome::Committed
    );
    assert!(reducer.scene().surfaces[0].current_state.maximized);
}

#[test]
fn presentation_geometry_and_focus_are_validated_against_output_roles() {
    let mut initial = scene(1, &[surface(1)]);
    initial.outputs[0].work_area = Rect {
        x: 0,
        y: 10,
        width: 100,
        height: 90,
    };
    let mut reducer = PolicyProjectionReducer::new(initial).unwrap();
    reducer.connect(1).unwrap();

    // Reaching above the work area into the shell's strut is no longer a
    // refusal: the Engine owns what is drawn there. An empty rectangle still
    // is one, because it names no pixels at all rather than pixels elsewhere.
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let above_work_area = placed(
        surface_id(1),
        1,
        Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        },
    );
    assert_eq!(
        reducer.apply_proposal(&proposal(
            &request,
            51,
            vec![projected(output(1), vec![above_work_area], None)],
        )),
        PolicyProjectionOutcome::Committed
    );

    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let empty = placed(
        surface_id(1),
        1,
        Rect {
            x: 0,
            y: 10,
            width: 0,
            height: 90,
        },
    );
    assert_eq!(
        reducer.apply_proposal(&proposal(
            &request,
            52,
            vec![projected(output(1), vec![empty], None)],
        )),
        PolicyProjectionOutcome::RejectedInvalid
    );

    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let mut fullscreen = placed(
        surface_id(1),
        1,
        Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        },
    );
    fullscreen.presentation.fullscreen = true;
    assert_eq!(
        reducer.apply_proposal(&proposal(
            &request,
            52,
            vec![projected(output(1), vec![fullscreen], Some(surface_id(1)),)],
        )),
        PolicyProjectionOutcome::Committed
    );

    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let mut minimized = placed(surface_id(1), 1, rect(0, 10));
    minimized.presentation.minimized = true;
    assert_eq!(
        reducer.apply_proposal(&proposal(
            &request,
            53,
            vec![projected(output(1), vec![minimized], Some(surface_id(1)),)],
        )),
        PolicyProjectionOutcome::RejectedInvalid
    );
}

#[test]
fn active_output_and_policy_generation_advance_explicitly() {
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[])).unwrap();
    reducer.connect(1).unwrap();
    assert!(reducer.admit_policy_generation(2).is_ok());
    assert!(reducer.admit_policy_generation(2).is_err());
    let request = reducer.issue_request(vec![output(1), output(2)]).unwrap();
    assert_eq!(request.policy_generation, 2);
    let mut candidate = proposal(
        &request,
        54,
        vec![
            projected(output(1), Vec::new(), None),
            projected(output(2), Vec::new(), None),
        ],
    );
    candidate.active_output = output(2);
    assert_eq!(
        reducer.apply_proposal(&candidate),
        PolicyProjectionOutcome::Committed
    );
    assert_eq!(reducer.scene().active_output, output(2));
}

fn scene(generation: u64, surfaces: &[PolicySurfaceSnapshot]) -> PolicySceneSnapshot {
    PolicySceneSnapshot {
        generation,
        active_output: output(1),
        outputs: vec![
            PolicyOutputSnapshot {
                policy_key: None,
                output: output(1),
                generation: 1,
                focus: None,
                bounds: Rect {
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 100,
                },
                work_area: Rect {
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 100,
                },
            },
            PolicyOutputSnapshot {
                policy_key: None,
                output: output(2),
                generation: 1,
                focus: None,
                bounds: Rect {
                    x: 100,
                    y: 0,
                    width: 100,
                    height: 100,
                },
                work_area: Rect {
                    x: 100,
                    y: 0,
                    width: 100,
                    height: 100,
                },
            },
        ],
        surfaces: surfaces.to_vec(),
        session_operations: Vec::new(),
    }
}

fn surface(raw: u32) -> PolicySurfaceSnapshot {
    PolicySurfaceSnapshot {
        surface: surface_id(raw),
        generation: 1,
        current_output: None,
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
        geometry: rect(0, 0),
    }
}

fn proposal(
    request: &sophia_protocol::PolicyProjectionRequest,
    transaction: u64,
    outputs: Vec<PolicyOutputProjection>,
) -> PolicyProjectionProposal {
    PolicyProjectionProposal {
        presentation: None,
        launch_contexts: Vec::new(),
        output_launch_contexts: Vec::new(),
        translation_groups: Vec::new(),
        tab_groups: Vec::new(),
        transaction: TransactionId::from_raw(transaction),
        connection_epoch: request.connection_epoch,
        request_id: request.request_id,
        base_generation: request.scene_generation,
        active_output: output(1),
        outputs,
        indicators: Vec::new(),
        output_statuses: Vec::new(),
    }
}

fn projected(
    output: OutputId,
    placements: Vec<PolicySurfacePlacement>,
    focus: Option<SurfaceId>,
) -> PolicyOutputProjection {
    PolicyOutputProjection {
        output,
        placements,
        focus,
    }
}

fn placed(surface: SurfaceId, generation: u64, geometry: Rect) -> PolicySurfacePlacement {
    PolicySurfacePlacement {
        surface,
        surface_generation: generation,
        geometry,
        requested_size: None,
        crop: None,
        transform: PolicyTransform::Identity,
        presentation: PolicyPresentationState::default(),
    }
}

fn output(raw: u64) -> OutputId {
    OutputId::from_raw(raw)
}

fn surface_id(index: u32) -> SurfaceId {
    SurfaceId::new(index, 1)
}

fn rect(x: i32, y: i32) -> Rect {
    Rect {
        x,
        y,
        width: 50,
        height: 50,
    }
}

/// A scene may describe a surface that no longer fits its output.
///
/// Shrinking a logical output -- optimizing a mirror group for its smaller
/// head, say -- leaves existing surfaces exactly where they were until the
/// policy answers with a new layout. Refusing to describe that instant ended a
/// live session: a surface tiled at 1280x1440 on a 2560x1440 group was still
/// 1440 tall when the group became 1080. Fit is required of a proposal, which
/// asserts a placement, not of a scene, which reports one.
#[test]
fn a_scene_may_report_a_surface_larger_than_its_shrunken_output() {
    let mut oversized = surface(1);
    oversized.geometry = Rect {
        x: 0,
        y: 0,
        width: 50,
        height: 400,
    };
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();

    reducer.observe_scene(scene(2, &[oversized])).unwrap();
}

/// A proposal may place a surface outside its output, because a scrolling
/// layout has to.
///
/// Columns scrolled past an edge keep their place in the strip and come back
/// when the camera moves. Refusing them made the policy pre-solve visibility —
/// a pixel question asked of a client that cannot see pixels — and made the
/// Engine dictate which layouts were expressible. Deciding what is drawn is
/// the Engine's job, and it can do that with the whole strip in hand.
#[test]
fn a_proposal_may_place_a_surface_outside_its_output() {
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();
    reducer.connect(1).unwrap();

    // Taller than the output, the shape the old rule refused.
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let overflowing = Rect {
        x: 0,
        y: 0,
        width: 50,
        height: 400,
    };
    assert_eq!(
        reducer.apply_proposal(&proposal(
            &request,
            7,
            vec![projected(
                output(1),
                vec![placed(surface_id(1), 1, overflowing)],
                None
            )],
        )),
        PolicyProjectionOutcome::Committed
    );

    // Scrolled off to the left, so its x is negative and its right edge is
    // still on screen. This is the ordinary state of a column behind the
    // camera.
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let scrolled_left = Rect {
        x: -30,
        y: 0,
        width: 50,
        height: 50,
    };
    assert_eq!(
        reducer.apply_proposal(&proposal(
            &request,
            8,
            vec![projected(
                output(1),
                vec![placed(surface_id(1), 1, scrolled_left)],
                None
            )],
        )),
        PolicyProjectionOutcome::Committed
    );
}

/// What replaced containment: a rectangle whose far edge cannot be computed.
///
/// Every consumer derives a right and bottom edge, so this is the invariant
/// that actually had to survive. A layout decision must not become an overflow
/// somewhere downstream.
#[test]
fn a_proposal_may_not_place_a_surface_whose_edges_overflow() {
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();
    reducer.connect(1).unwrap();
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let unrepresentable = Rect {
        x: i32::MAX - 10,
        y: 0,
        width: 50,
        height: 50,
    };

    assert_eq!(
        reducer.apply_proposal(&proposal(
            &request,
            7,
            vec![projected(
                output(1),
                vec![placed(surface_id(1), 1, unrepresentable)],
                None
            )],
        )),
        PolicyProjectionOutcome::RejectedInvalid
    );
}

/// A commit that leaves the indicators alone leaves the publication alone.
///
/// Consumers compare the whole publication to decide whether to re-raster the
/// strip, re-damage it, and keep an in-flight click alive. While it carried a
/// commit serial, every policy commit looked like new indicator content, which
/// turned an ordinary layout commit into a full recomposition.
#[test]
fn an_unchanged_indicator_publication_survives_a_policy_commit() {
    fn descriptors(label: &str) -> (PolicyProjectionIndicator, PolicyProjectionOutputStatus) {
        (
            PolicyProjectionIndicator {
                output: output(1),
                slot: 0,
                indicator: 9,
                action: Some(WmActionId::from_raw(11)),
                state_bits: 1,
                label: label.into(),
            },
            PolicyProjectionOutputStatus {
                output: output(1),
                focus_bits: 0,
                layout: "Scroller".into(),
            },
        )
    }

    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1)])).unwrap();
    reducer.connect(5).unwrap();

    let commit = |reducer: &mut PolicyProjectionReducer, transaction: u64, label: &str| {
        let request = reducer.issue_request(vec![output(1)]).unwrap();
        let mut candidate = proposal(
            &request,
            transaction,
            vec![projected(output(1), Vec::new(), None)],
        );
        let (indicator, status) = descriptors(label);
        candidate.indicators.push(indicator);
        candidate.output_statuses.push(status);
        assert_eq!(
            reducer.apply_proposal(&candidate),
            PolicyProjectionOutcome::Committed
        );
    };

    commit(&mut reducer, 61, "1");
    let published = reducer.indicator_publication();

    // A second commit carrying the same descriptors.
    commit(&mut reducer, 62, "1");
    assert_eq!(
        reducer.indicator_publication(),
        published,
        "an unchanged publication must compare equal across commits"
    );
    // The serial still counts every commit; it guards settlement staleness.
    assert_eq!(reducer.commit_serial(), 2);

    // Changed content still advances the generation.
    commit(&mut reducer, 63, "2");
    let moved = reducer.indicator_publication();
    assert!(moved.generation > published.generation);
    assert_eq!(moved.indicators[0].label, "2");
}

#[test]
fn tab_groups_commit_with_geometry_and_hidden_members_remain_scene_owned() {
    let mut reducer = PolicyProjectionReducer::new(scene(1, &[surface(1), surface(2)])).unwrap();
    reducer.connect(1).unwrap();
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    let mut p = proposal(
        &request,
        91,
        vec![projected(
            output(1),
            vec![placed(surface_id(1), 1, rect(0, 24))],
            Some(surface_id(1)),
        )],
    );
    p.tab_groups = vec![sophia_protocol::PolicyTabGroup {
        output: output(1),
        group: 1,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        },
        focused: true,
        selected: Some(surface_id(1)),
        members: vec![surface_id(1), surface_id(2)],
    }];
    let staged = reducer.stage_proposal(&p).unwrap();
    assert!(reducer.indicator_publication().tab_groups.is_empty());
    assert_eq!(
        reducer.commit_staged(staged),
        PolicyProjectionOutcome::Committed
    );
    assert_eq!(reducer.indicator_publication().tab_groups, p.tab_groups);
    assert_eq!(reducer.committed()[0].placements.len(), 1);
    let before = reducer.indicator_publication();
    let request = reducer.issue_request(vec![output(1)]).unwrap();
    p.request_id = request.request_id;
    p.transaction = TransactionId::from_raw(92);
    p.tab_groups[0].members.push(surface_id(3));
    assert_eq!(
        reducer.apply_proposal(&p),
        PolicyProjectionOutcome::RejectedInvalid
    );
    assert_eq!(reducer.indicator_publication(), before);
    reducer.disconnect(1);
    assert!(reducer.indicator_publication().tab_groups.is_empty());
}

#[test]
fn pointer_focus_requires_a_live_focusable_target_on_its_affected_output() {
    let mut window = surface(1);
    window.current_output = Some(output(1));
    for (destination, target, affected, focusable, admitted) in [
        (1, Some(surface_id(1)), vec![output(1)], true, true),
        (2, None, vec![output(1), output(2)], true, true),
        (
            2,
            Some(surface_id(1)),
            vec![output(1), output(2)],
            true,
            false,
        ),
        (1, Some(surface_id(1)), vec![output(1)], false, false),
        (1, Some(SurfaceId::new(1, 9)), vec![output(1)], true, false),
        (2, None, vec![output(1)], true, false),
        (3, None, vec![output(1)], true, false),
    ] {
        window.capabilities.focusable = focusable;
        let mut reducer = PolicyProjectionReducer::new(scene(1, &[window])).unwrap();
        reducer.connect(1).unwrap();
        let before = reducer.scene().clone();
        let result = reducer.issue_request_with_cause(
            affected,
            PolicyRequestCause::PointerFocus {
                output: output(destination),
                target,
            },
        );
        assert_eq!(result.is_ok(), admitted);
        assert_eq!(reducer.scene(), &before);
        assert_eq!(reducer.commit_serial(), 0);
    }
}

#[test]
fn output_action_requires_exact_current_target_and_coverage() {
    for (target, generation, coverage, accepted) in [
        (output(2), 1, vec![output(1), output(2)], true),
        (output(2), 2, vec![output(1), output(2)], false),
        (output(2), 1, vec![output(1)], false),
        (output(99), 1, vec![output(1), output(2)], false),
    ] {
        let mut reducer = PolicyProjectionReducer::new(scene(1, &[])).unwrap();
        reducer.connect(1).unwrap();
        let result = reducer.issue_request_with_cause(
            coverage,
            PolicyRequestCause::OutputAction {
                activation_serial: 1,
                action: WmActionId::from_raw(15),
                output: target,
                output_generation: generation,
            },
        );
        assert_eq!(result.is_ok(), accepted);
        assert_eq!(reducer.outstanding().is_some(), accepted);
    }
}
