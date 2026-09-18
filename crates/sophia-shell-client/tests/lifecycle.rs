use sophia_protocol::*;
use sophia_shell_client::*;

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 2,
    }
}
fn output() -> ContentOutputId {
    ContentOutputId {
        id: 3,
        generation: 4,
    }
}
fn candidate(generation: u64) -> ClientContentCandidate {
    ClientContentCandidate {
        transaction: TransactionId::from_raw(generation),
        begin: ContentCandidateBegin {
            grant: grant(),
            output: output(),
            candidate_generation: generation,
            facts_generation: 1,
            pacing_permit: generation,
            interaction_generation: 1,
            surface_count: 1,
            placement_count: 1,
            target_count: 1,
        },
        surfaces: vec![ContentSurface {
            allocation: ContentAllocationId {
                id: 5,
                generation: 6,
            },
            scale_generation: 1,
            role: 1,
            edge: 1,
            margins: ContentMargins::default(),
            reservation_extent: 0,
            parent_surface_index: u16::MAX,
            anchor_parent_rect: ContentPixelRect::default(),
        }],
        targets: vec![ContentTarget {
            surface_index: 0,
            action_kind: 1,
            target_id: 7,
            target_generation: generation,
            action_id: 8,
            bounds_px: ContentPixelRect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
        }],
    }
}
fn outcome(generation: u64, kind: u16) -> ShellContentRecord {
    ShellContentRecord::CandidateOutcome(ContentCandidateOutcome {
        grant: grant(),
        output: output(),
        candidate_generation: generation,
        kind,
        reason: 0,
        presentation_epoch: if kind == 2 { generation + 10 } else { 0 },
        work_area_generation: 1,
        wm_commit_generation: 1,
    })
}
fn action(generation: u64, event: u64, kind: u16) -> ShellContentRecord {
    ShellContentRecord::Action(ContentAction {
        grant: grant(),
        output: output(),
        candidate_generation: generation,
        presentation_epoch: generation + 10,
        interaction_generation: 1,
        allocation: candidate(generation).surfaces[0].allocation,
        target_id: 7,
        target_generation: generation,
        action_id: 8,
        event_id: event,
        kind,
        reason: 0,
    })
}
fn dispatch(lifecycle: &mut ContentLifecycle, record: ShellContentRecord) -> ContentDispatch {
    lifecycle
        .dispatch(TransactionId::from_raw(90), record)
        .unwrap()
}

#[test]
fn all_target_classes_match_activation_events_without_promoting_prepared() {
    for class in 1..=3 {
        let mut lifecycle = ContentLifecycle::new(ContentLimits::prototype(grant())).unwrap();
        let mut value = candidate(1);
        value.targets[0].action_kind = class;
        lifecycle.register(value).unwrap();
        lifecycle
            .dispatch(TransactionId::from_raw(1), outcome(1, 1))
            .unwrap();
        assert_eq!(
            dispatch(&mut lifecycle, action(1, 1, 1)).action,
            Some(ContentActionDispatch::Rejected)
        );
        lifecycle
            .dispatch(TransactionId::from_raw(1), outcome(1, 2))
            .unwrap();
        assert_eq!(
            dispatch(&mut lifecycle, action(1, 2, 1)).action,
            Some(ContentActionDispatch::Eligible)
        );
        assert_eq!(
            dispatch(&mut lifecycle, action(1, 2, 1)).action,
            Some(ContentActionDispatch::Rejected)
        );
    }
}

#[test]
fn prepared_never_installs_targets_and_presented_does_not_wait_for_release() {
    let mut lifecycle = ContentLifecycle::new(ContentLimits::prototype(grant())).unwrap();
    lifecycle.register(candidate(1)).unwrap();
    let prepared = lifecycle
        .dispatch(TransactionId::from_raw(1), outcome(1, 1))
        .unwrap();
    assert_eq!(prepared.transaction.raw(), 1);
    assert!(lifecycle.presented(output()).is_none());
    assert_eq!(
        dispatch(&mut lifecycle, action(1, 1, 1)).action,
        Some(ContentActionDispatch::Rejected)
    );
    lifecycle
        .dispatch(TransactionId::from_raw(1), outcome(1, 2))
        .unwrap();
    assert_eq!(
        dispatch(&mut lifecycle, action(1, 2, 1)).action,
        Some(ContentActionDispatch::Eligible)
    );
    assert_eq!(
        lifecycle.presented(output()).unwrap().presentation_epoch,
        11
    );
    // No ResourceReleased record has been needed for target publication.
}

#[test]
fn replacement_removes_old_targets_but_retains_the_old_action_for_cancel() {
    let mut lifecycle = ContentLifecycle::new(ContentLimits::prototype(grant())).unwrap();
    lifecycle.register(candidate(1)).unwrap();
    lifecycle
        .dispatch(TransactionId::from_raw(1), outcome(1, 2))
        .unwrap();
    assert_eq!(
        dispatch(&mut lifecycle, action(1, 1, 1)).action,
        Some(ContentActionDispatch::Eligible)
    );
    lifecycle.register(candidate(2)).unwrap();
    lifecycle
        .dispatch(TransactionId::from_raw(2), outcome(2, 2))
        .unwrap();
    assert_eq!(
        dispatch(&mut lifecycle, action(1, 2, 1)).action,
        Some(ContentActionDispatch::Rejected)
    );
    assert_eq!(
        dispatch(&mut lifecycle, action(1, 1, 3)).action,
        Some(ContentActionDispatch::Cancelled)
    );
    assert_eq!(
        dispatch(&mut lifecycle, action(2, 3, 1)).action,
        Some(ContentActionDispatch::Eligible)
    );
    // Cancel is dispatched by kind even after its event number was observed.
    assert_eq!(
        dispatch(&mut lifecycle, action(2, 3, 3)).action,
        Some(ContentActionDispatch::Cancelled)
    );
    assert_eq!(
        dispatch(&mut lifecycle, action(2, 3, 3)).action,
        Some(ContentActionDispatch::Cancelled)
    );
}

#[test]
fn outcome_transaction_and_grant_mismatch_cannot_change_current_targets() {
    let mut lifecycle = ContentLifecycle::new(ContentLimits::prototype(grant())).unwrap();
    lifecycle.register(candidate(1)).unwrap();
    assert!(matches!(
        lifecycle.dispatch(TransactionId::from_raw(2), outcome(1, 2)),
        Err(ContentLifecycleError::WrongTransaction)
    ));
    assert!(lifecycle.presented(output()).is_none());
    let mut wrong = outcome(1, 2);
    if let ShellContentRecord::CandidateOutcome(value) = &mut wrong {
        value.grant.content_grant_epoch += 1;
    }
    assert!(matches!(
        lifecycle.dispatch(TransactionId::from_raw(1), wrong),
        Err(ContentLifecycleError::WrongGrant)
    ));
    lifecycle
        .dispatch(TransactionId::from_raw(1), outcome(1, 2))
        .unwrap();
    assert!(lifecycle.register(candidate(1)).is_err());
}

fn facts(generation: u64, output: ContentOutputId, scale_generation: u64) -> ShellContentRecord {
    ShellContentRecord::OutputFacts(ContentOutputFacts {
        grant: grant(),
        facts_generation: generation,
        outputs: vec![ContentOutputFactsEntry {
            output,
            local_width: 100,
            local_height: 100,
            scale_numerator: 1,
            scale_denominator: 1,
            scale_generation,
        }],
    })
}

#[test]
fn topology_replacement_cannot_reactivate_an_old_pending_candidate() {
    for change_scale in [false, true] {
        let mut lifecycle = ContentLifecycle::new(ContentLimits::prototype(grant())).unwrap();
        dispatch(&mut lifecycle, facts(1, output(), 1));
        lifecycle.register(candidate(1)).unwrap();
        lifecycle
            .dispatch(TransactionId::from_raw(1), outcome(1, 2))
            .unwrap();
        lifecycle.register(candidate(2)).unwrap();
        let replacement = ContentOutputId {
            generation: output().generation + u64::from(!change_scale),
            ..output()
        };
        dispatch(
            &mut lifecycle,
            facts(2, replacement, if change_scale { 2 } else { 1 }),
        );
        assert!(lifecycle.presented(output()).is_none());
        assert!(matches!(
            lifecycle.dispatch(TransactionId::from_raw(2), outcome(2, 2)),
            Err(ContentLifecycleError::InvalidOutcome)
        ));
        assert_eq!(
            dispatch(&mut lifecycle, action(2, 1, 1)).action,
            Some(ContentActionDispatch::Rejected)
        );
        // A terminal refusal can still discharge old pending metadata.
        lifecycle
            .dispatch(TransactionId::from_raw(2), outcome(2, 3))
            .unwrap();
        assert!(lifecycle.register(candidate(3)).is_err());
    }
}

#[test]
fn stale_facts_and_invalid_prepared_do_not_change_current_targets() {
    let mut lifecycle = ContentLifecycle::new(ContentLimits::prototype(grant())).unwrap();
    dispatch(&mut lifecycle, facts(2, output(), 1));
    lifecycle.register(candidate(1)).unwrap();
    lifecycle
        .dispatch(TransactionId::from_raw(1), outcome(1, 2))
        .unwrap();
    lifecycle.register(candidate(2)).unwrap();
    let mut prepared = outcome(2, 1);
    if let ShellContentRecord::CandidateOutcome(value) = &mut prepared {
        value.presentation_epoch = 99;
    }
    assert!(matches!(
        lifecycle.dispatch(TransactionId::from_raw(2), prepared),
        Err(ContentLifecycleError::InvalidOutcome)
    ));
    assert!(matches!(
        lifecycle.dispatch(TransactionId::from_raw(90), facts(1, output(), 2)),
        Err(ContentLifecycleError::InvalidOutcome)
    ));
    assert_eq!(
        dispatch(&mut lifecycle, action(1, 1, 1)).action,
        Some(ContentActionDispatch::Eligible)
    );
}
