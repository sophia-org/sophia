use super::super::{
    LiveOutputTopologyExecutionPhase, LiveOutputTopologyOwner, LiveOutputTopologyPhase,
    LiveOutputTopologyQuarantine, LiveOutputTopologyRebuild,
    begin_output_topology_first_presentation_rollback, hardware_output_snapshot_is_stale,
    owner_loop_shell_presentation_available, schedule_topology_repaint,
};
use crate::live_session::desktop_profile_reload_effects;
use sophia_protocol::{OutputId, Size, TransactionId};
use std::cell::RefCell;

/// Rebuild with one head per output: the ordinary unmirrored desktop, and every
/// case here except the group that loses a connector.
fn observe_unmirrored(
    owner: &mut LiveOutputTopologyOwner,
    outputs: Vec<sophia_engine::HeadlessOutput>,
) -> Result<LiveOutputTopologyRebuild, &'static str> {
    let heads = outputs.iter().map(|output| (output.id, 1)).collect();
    owner.observe_rebuild(outputs, heads)
}

/// The policy candidate's rebuild. Distinct from `observe_unmirrored` because
/// the two are distinct writers: a candidate may not consume the rescan path's
/// quarantine, nor a rescan the candidate's.
fn observe_policy_unmirrored(
    owner: &mut LiveOutputTopologyOwner,
    outputs: Vec<sophia_engine::HeadlessOutput>,
    candidate_topology_epoch: u64,
) -> Result<(), &'static str> {
    let heads = outputs.iter().map(|output| (output.id, 1)).collect();
    owner.observe_policy_rebuild(outputs, heads, candidate_topology_epoch)
}

fn output(raw: u64, width: i32) -> sophia_engine::HeadlessOutput {
    sophia_engine::HeadlessOutput {
        id: OutputId::from_raw(raw),
        size: Size { width, height: 720 },
        scale: 1,
    }
}

fn owner() -> LiveOutputTopologyOwner {
    LiveOutputTopologyOwner::new_at_generation(
        vec![output(1, 1280)],
        vec![(OutputId::from_raw(1), 1)],
        1,
    )
    .unwrap()
}

#[test]
fn topology_repaint_survives_backpressure_without_crediting_a_presentation() {
    use sophia_backend_live::LiveProductionCpuScene;
    use sophia_engine::PrimaryFramePacer;
    use std::time::{Duration, Instant};

    for policy_required in [false, true] {
        let mut owner = owner();
        owner.begin_rescan(1).unwrap();
        observe_unmirrored(&mut owner, vec![output(1, 1280), output(2, 1920)]).unwrap();
        owner.mark_published(7, policy_required).unwrap();
        if policy_required {
            owner.mark_policy_committed(7).unwrap();
        }
        let mut scene = LiveProductionCpuScene::new(output(1, 1280).size);
        let interval = Duration::from_millis(10);
        let now = Instant::now();
        let mut pacer = PrimaryFramePacer::new(interval);
        pacer.observe_repaint(now);

        schedule_topology_repaint(&mut scene, &mut pacer, now);
        assert!(pacer.repaint_pending());
        assert!(!pacer.repaint_due(now));
        for turn in 1..=3 {
            let due = now + interval * turn;
            assert!(pacer.repaint_due(due));
            // The ordinary path returns None while a distinct frame owns
            // retirement. Its scheduling response must keep the obligation
            // without a zero wait, or releasing topology input/publication.
            pacer.observe_repaint_deferred(due);
            assert!(pacer.repaint_pending());
            assert_eq!(pacer.cap_wait(due, Duration::from_secs(1)), interval);
            assert!(!owner.observe_presentation(7));
            assert!(owner.input_quarantined());
        }

        let ready = now + interval * 4;
        assert!(pacer.repaint_due(ready), "no new client event is needed");
        pacer.observe_repaint(ready);
        assert!(!pacer.repaint_pending());
        assert!(
            !owner.observe_presentation(7),
            "composition is not retirement"
        );
        assert!(owner.input_quarantined());
        assert!(owner.observe_presentation(8));
        assert!(!owner.input_quarantined());
        assert!(!owner.observe_presentation(8));
    }
}

#[test]
fn lock_coverage_does_not_name_the_previous_topology_during_a_rebind() {
    use crate::session_lock_coverage::SessionLockCoveragePublication;
    let epoch = sophia_engine::SessionLockEpoch::from_raw(3).unwrap();
    let mut publication = SessionLockCoveragePublication::default();
    let mut owner = owner();
    assert_eq!(owner.settled_coverage_epoch(Some(1), false, false), Some(1));
    assert!(owner.begin_rescan(1).unwrap());
    assert_eq!(owner.settled_coverage_epoch(Some(1), false, false), None);
    observe_unmirrored(&mut owner, vec![output(1, 1920), output(2, 1280)]).unwrap();
    owner.mark_published(7, false).unwrap();
    // The new heads have retired a cover, but the old public epoch still
    // names the previous topology. A first report must not misattribute it.
    assert!(
        publication
            .update(
                Some(epoch),
                owner.settled_coverage_epoch(Some(1), true, false),
                Some(7),
                Some((epoch, 2, 2)),
            )
            .is_none()
    );
    assert_eq!(owner.settled_coverage_epoch(Some(2), false, false), None);
    assert!(owner.observe_presentation(8));
    for (published, pending, candidate) in [
        (None, false, false),
        (Some(1), false, false),
        (Some(3), false, false),
        (Some(2), true, false),
        (Some(2), false, true),
    ] {
        assert_eq!(
            owner.settled_coverage_epoch(published, pending, candidate),
            None
        );
    }
    let record = publication
        .update(
            Some(epoch),
            owner.settled_coverage_epoch(Some(2), false, false),
            Some(7),
            Some((epoch, 2, 2)),
        )
        .unwrap();
    assert_eq!(
        (record.topology_epoch, record.outputs, record.heads),
        (2, 2, 2)
    );
}

#[test]
fn changed_topology_advances_public_identity_once() {
    let mut owner = owner();
    assert_eq!(owner.begin_rescan(1), Ok(true));
    assert_eq!(
        observe_unmirrored(&mut owner, vec![output(1, 1920), output(2, 1280)]),
        Ok(LiveOutputTopologyRebuild::TopologyChanged)
    );
    assert_eq!(owner.topology_epoch, 2);
    assert_eq!(owner.publication_generation, 2);
    owner.mark_published(7, true).unwrap();
    assert!(!owner.observe_presentation(8));
    owner.mark_policy_committed(8).unwrap();
    assert!(!owner.observe_presentation(7));
    assert!(!owner.observe_presentation(8));
    assert!(owner.observe_presentation(9));
    assert!(!owner.input_quarantined());
}

#[test]
fn losing_one_head_of_a_mirror_group_is_a_new_candidate() {
    // A group that loses a connector keeps its logical output, so the output list
    // is byte-for-byte what it was. Comparing that alone would republish nothing
    // and leave consumers holding an epoch computed for the wider group -- the
    // case `PublishedHeadsAreCurrent` exists to forbid.
    let outputs = vec![output(1, 1280)];
    let mut owner = LiveOutputTopologyOwner::new_at_generation(
        outputs.clone(),
        vec![(OutputId::from_raw(1), 2)],
        1,
    )
    .unwrap();

    assert!(owner.begin_rescan(1).unwrap());
    assert_eq!(
        owner.observe_rebuild(outputs.clone(), vec![(OutputId::from_raw(1), 1)]),
        Ok(LiveOutputTopologyRebuild::TopologyChanged)
    );
    assert_eq!(owner.topology_epoch, 2);
    assert_eq!(owner.publication_generation, 2);

    // And an unchanged group is still no change, so the head count cannot make
    // every rescan look like a new topology.
    owner.mark_published(1, false).unwrap();
    assert!(owner.observe_presentation(2));
    assert!(owner.begin_rescan(2).unwrap());
    assert_eq!(
        owner.observe_rebuild(outputs, vec![(OutputId::from_raw(1), 1)]),
        Ok(LiveOutputTopologyRebuild::TransportReplaced)
    );
    assert_eq!(owner.topology_epoch, 2);
}

#[test]
fn configured_initial_publication_generation_advances_from_its_baseline() {
    let mut owner = LiveOutputTopologyOwner::new_at_generation(
        vec![output(1, 1280)],
        vec![(OutputId::from_raw(1), 1)],
        2,
    )
    .unwrap();
    owner.begin_rescan(1).unwrap();
    observe_unmirrored(&mut owner, vec![output(1, 1920)]).unwrap();
    assert_eq!(owner.publication_generation, 3);
}

#[test]
fn retry_does_not_consume_another_security_or_public_epoch() {
    let mut owner = owner();
    assert_eq!(owner.begin_rescan(4), Ok(true));
    assert_eq!(
        observe_unmirrored(&mut owner, Vec::new()),
        Ok(LiveOutputTopologyRebuild::Unavailable)
    );
    assert_eq!(owner.begin_rescan(5), Ok(false));
    assert_eq!(owner.topology_epoch, 1);
    assert_eq!(owner.publication_generation, 1);
    assert_eq!(
        observe_unmirrored(&mut owner, vec![output(1, 1280)]),
        Ok(LiveOutputTopologyRebuild::TransportReplaced)
    );
}

#[test]
fn transport_replacement_still_waits_for_new_presentation() {
    let mut owner = owner();
    owner.begin_rescan(1).unwrap();
    observe_unmirrored(&mut owner, vec![output(1, 1280)]).unwrap();
    owner.mark_published(11, false).unwrap();
    assert!(!owner.observe_presentation(11));
    assert!(owner.observe_presentation(12));
}

#[test]
fn newer_notice_restarts_publication_without_reconsuming_security_epoch() {
    let mut owner = owner();
    assert!(owner.begin_rescan(1).unwrap());
    observe_unmirrored(&mut owner, vec![output(1, 1920)]).unwrap();
    owner.mark_published(3, true).unwrap();
    assert!(!owner.begin_rescan(2).unwrap());
    assert_eq!(
        owner.phase,
        LiveOutputTopologyPhase::Quarantined(LiveOutputTopologyQuarantine::Hotplug)
    );
    assert_eq!(owner.transition, 2);
}

#[test]
fn redundant_notice_cannot_bypass_pending_policy_settlement() {
    let mut owner = owner();
    owner.begin_rescan(1).unwrap();
    observe_unmirrored(&mut owner, vec![output(1, 1920)]).unwrap();
    owner.mark_published(3, true).unwrap();
    owner.begin_rescan(2).unwrap();
    observe_unmirrored(&mut owner, vec![output(1, 1920)]).unwrap();
    owner.mark_published(4, false).unwrap();
    assert_eq!(owner.phase, LiveOutputTopologyPhase::Published);
    assert!(owner.policy_settlement_pending);
}

#[test]
fn duplicate_coalescer_token_does_not_restart_a_transition() {
    let mut owner = owner();
    assert!(owner.begin_rescan(3).unwrap());
    assert!(!owner.begin_rescan(3).unwrap());
    assert_eq!(owner.transition, 1);
    assert_eq!(
        owner.phase,
        LiveOutputTopologyPhase::Quarantined(LiveOutputTopologyQuarantine::Hotplug)
    );
}

#[test]
fn policy_change_keeps_published_identity_private_until_commit() {
    let mut owner = owner();
    assert_eq!(owner.begin_policy_change(), Ok(true));
    assert!(owner.input_quarantined());
    assert_eq!(owner.topology_epoch, 1);
    assert_eq!(owner.publication_generation, 1);
    assert_eq!(owner.outputs, vec![output(1, 1280)]);

    // Through the policy writer, not the rescan one. This test previously drove
    // `observe_rebuild` here, which is the hotplug path, and that mixing is what
    // let a rescan consume a candidate's quarantine in a live session.
    assert_eq!(
        observe_policy_unmirrored(&mut owner, vec![output(1, 1920), output(2, 1280)], 2),
        Ok(()),
    );
    assert_eq!(owner.topology_epoch, 2);
    assert_eq!(owner.publication_generation, 2);
    owner.mark_published(8, false).unwrap();
    assert!(owner.observe_presentation(9));
}

#[test]
fn rejected_policy_change_restores_stable_without_consuming_public_identity() {
    let mut owner = owner();
    owner.begin_policy_change().unwrap();
    owner.cancel_policy_change().unwrap();
    assert_eq!(owner.phase, LiveOutputTopologyPhase::Stable);
    assert_eq!(owner.topology_epoch, 1);
    assert_eq!(owner.publication_generation, 1);
    assert_eq!(owner.outputs, vec![output(1, 1280)]);
}

#[test]
fn frontend_candidate_rollback_consumes_only_transport_generations() {
    let mut owner = owner();
    owner.begin_policy_change().unwrap();
    owner.observe_policy_transport_rollback(3).unwrap();
    owner.cancel_policy_change().unwrap();
    assert_eq!(owner.phase, LiveOutputTopologyPhase::Stable);
    assert_eq!(owner.topology_epoch, 1);
    assert_eq!(owner.publication_generation, 3);
    assert_eq!(owner.outputs, vec![output(1, 1280)]);
}

#[test]
fn policy_commit_advances_epoch_when_logical_shape_is_unchanged() {
    let mut owner = owner();
    owner.begin_policy_change().unwrap();
    owner
        .observe_policy_rebuild(vec![output(1, 1280)], vec![(OutputId::from_raw(1), 1)], 2)
        .unwrap();
    assert_eq!(owner.topology_epoch, 2);
    assert_eq!(owner.publication_generation, 2);
    owner.mark_published(4, false).unwrap();
    assert!(owner.observe_presentation(5));
}

#[test]
fn shell_reconnect_waits_until_topology_execution_fully_clears() {
    for phase in [
        LiveOutputTopologyExecutionPhase::WaitingForQuiescence,
        LiveOutputTopologyExecutionPhase::Preparing,
        LiveOutputTopologyExecutionPhase::Applying,
        LiveOutputTopologyExecutionPhase::AwaitingFirstPresentation,
        LiveOutputTopologyExecutionPhase::Reconciling,
        LiveOutputTopologyExecutionPhase::RollingBack,
    ] {
        assert!(
            !owner_loop_shell_presentation_available(true, true, Some(phase), false),
            "native readiness during {phase:?} must not publish stale output facts"
        );
    }
    assert!(owner_loop_shell_presentation_available(
        true, true, None, false
    ));
}

#[test]
fn first_presentation_service_failure_orders_physical_rollback_before_policy_rejection() {
    let mut phase = LiveOutputTopologyExecutionPhase::AwaitingFirstPresentation;
    let transaction = TransactionId::from_raw(41);
    let effects = RefCell::new(Vec::new());

    assert!(
        begin_output_topology_first_presentation_rollback(
            &mut phase,
            transaction,
            "renderer worker refused frame",
            |reason| {
                effects.borrow_mut().push(format!("native:{reason}"));
                Ok(())
            },
            |observed| {
                effects
                    .borrow_mut()
                    .push(format!("policy:{}", observed.raw()));
                Ok(())
            },
        )
        .unwrap()
    );
    assert_eq!(phase, LiveOutputTopologyExecutionPhase::RollingBack);
    assert_eq!(
        effects.into_inner(),
        vec![
            "native:first topology presentation failed: renderer worker refused frame",
            "policy:41",
        ]
    );
}

#[test]
fn native_service_failure_outside_first_presentation_remains_fatal() {
    let mut phase = LiveOutputTopologyExecutionPhase::Applying;
    let effects = RefCell::new(Vec::new());

    assert!(
        !begin_output_topology_first_presentation_rollback(
            &mut phase,
            TransactionId::from_raw(42),
            "unrelated failure",
            |reason| {
                effects.borrow_mut().push(reason);
                Ok(())
            },
            |transaction| {
                effects.borrow_mut().push(transaction.raw().to_string());
                Ok(())
            },
        )
        .unwrap()
    );
    assert_eq!(phase, LiveOutputTopologyExecutionPhase::Applying);
    assert!(effects.into_inner().is_empty());
}

#[test]
fn policy_failure_retains_the_physically_accepted_rollback_phase() {
    let mut phase = LiveOutputTopologyExecutionPhase::AwaitingFirstPresentation;
    let error = begin_output_topology_first_presentation_rollback(
        &mut phase,
        TransactionId::from_raw(43),
        "export failed",
        |_| Ok(()),
        |_| Err("policy transport disconnected".into()),
    )
    .unwrap_err();

    assert_eq!(phase, LiveOutputTopologyExecutionPhase::RollingBack);
    assert_eq!(error.to_string(), "policy transport disconnected");
}

/// A policy candidate's quarantine is not the rescan path's to consume.
///
/// Sharing one untagged `Quarantined` phase between the two writers meant a
/// hotplug rebuild ran to completion on a candidate's quarantine and released
/// it, so the candidate reached `observe_policy_rebuild` to find the owner
/// already `Stable` and failed a live session mid-apply.
#[test]
fn a_hotplug_rebuild_cannot_consume_a_policy_quarantine() {
    let mut owner = LiveOutputTopologyOwner::new_at_generation(
        vec![output(1, 1920)],
        vec![(OutputId::from_raw(1), 1)],
        1,
    )
    .unwrap();

    owner.begin_policy_change().unwrap();
    assert_eq!(
        owner.phase,
        LiveOutputTopologyPhase::Quarantined(LiveOutputTopologyQuarantine::Policy)
    );

    // A notice arriving now is remembered, not serviced.
    assert!(!owner.begin_rescan(1).unwrap());
    assert_eq!(
        owner.phase,
        LiveOutputTopologyPhase::Quarantined(LiveOutputTopologyQuarantine::Policy)
    );

    // The rescan path is refused outright rather than silently taking over.
    assert!(
        owner
            .observe_rebuild(vec![output(1, 2560)], vec![(OutputId::from_raw(1), 1)])
            .is_err()
    );
    assert_eq!(
        owner.phase,
        LiveOutputTopologyPhase::Quarantined(LiveOutputTopologyQuarantine::Policy)
    );

    // The candidate still owns its quarantine and can complete.
    owner
        .observe_policy_rebuild(vec![output(1, 2560)], vec![(OutputId::from_raw(1), 1)], 2)
        .unwrap();
    assert_eq!(owner.phase, LiveOutputTopologyPhase::Rebuilt);
}

/// The deferred notice is re-armed once the candidate settles, so a hotplug
/// that arrived at the wrong moment is delayed rather than dropped.
#[test]
fn a_notice_deferred_by_a_policy_candidate_is_rearmed_when_it_settles() {
    let mut owner = LiveOutputTopologyOwner::new_at_generation(
        vec![output(1, 1920)],
        vec![(OutputId::from_raw(1), 1)],
        1,
    )
    .unwrap();

    owner.begin_policy_change().unwrap();
    assert!(!owner.begin_rescan(1).unwrap());
    // Still quarantined, so nothing is owed yet.
    assert!(!owner.take_deferred_hotplug_notice());

    owner.cancel_policy_change().unwrap();
    assert_eq!(owner.phase, LiveOutputTopologyPhase::Stable);
    assert!(owner.take_deferred_hotplug_notice());
    // Claimed exactly once.
    assert!(!owner.take_deferred_hotplug_notice());
}

/// The post-commit presentation wait must be escapable.
///
/// It holds input at shortcuts-only until the committed layout reaches a
/// screen, but nothing forces that frame: a relayout that moves nothing
/// produces no damage and so no flip. That case is indistinguishable from a
/// slow client, and in it the displayed layout is already the committed one, so
/// waiting forever protects nothing while the desktop feels dead.
#[test]
fn a_presentation_wait_can_be_released_without_its_flip() {
    let mut owner = LiveOutputTopologyOwner::new_at_generation(
        vec![output(1, 1280)],
        vec![(OutputId::from_raw(1), 1)],
        1,
    )
    .unwrap();

    owner.begin_policy_change().unwrap();
    observe_policy_unmirrored(&mut owner, vec![output(1, 1920)], 2).unwrap();
    owner.mark_published(8, true).unwrap();
    assert_eq!(owner.phase, LiveOutputTopologyPhase::Published);

    // Not released before the policy commits: that would restore input while
    // the layout it is waiting on is still unsettled.
    assert!(!owner.release_presentation_wait());

    owner.mark_policy_committed(9).unwrap();
    assert_eq!(owner.phase, LiveOutputTopologyPhase::AwaitingPresentation);
    assert!(owner.input_quarantined());
    // No flip arrives: retirements never exceed the baseline.
    assert!(!owner.observe_presentation(9));

    assert!(owner.release_presentation_wait());
    assert_eq!(owner.phase, LiveOutputTopologyPhase::Stable);
    assert!(!owner.input_quarantined());

    // Claimed once; a second call is not a second release.
    assert!(!owner.release_presentation_wait());
    // Releasing input is not evidence of a presentation. The actual late
    // completion still releases the parked hardware publication, once.
    assert!(!owner.observe_presentation(9));
    assert!(owner.observe_presentation(10));
    assert!(!owner.observe_presentation(11));
}

#[test]
fn a_new_notice_invalidates_a_timed_out_owners_late_presentation() {
    let mut owner = owner();
    owner.begin_rescan(1).unwrap();
    observe_unmirrored(&mut owner, vec![output(1, 1920)]).unwrap();
    owner.mark_published(8, false).unwrap();
    assert!(owner.release_presentation_wait());
    owner.begin_rescan(2).unwrap();
    assert!(!owner.observe_presentation(9));
    observe_unmirrored(&mut owner, vec![output(1, 1920)]).unwrap();
    owner.mark_published(0, false).unwrap();
    assert!(!owner.observe_presentation(0));
    assert!(owner.observe_presentation(1));
}

#[test]
fn connector_or_policy_change_advances_topology_even_with_identical_output_numbers_and_sizes() {
    let mut owner = owner();
    owner.begin_rescan(1).unwrap();
    assert_eq!(
        owner
            .observe_resolved_rebuild(
                vec![output(1, 1280)],
                vec![(OutputId::from_raw(1), 1)],
                true
            )
            .unwrap(),
        LiveOutputTopologyRebuild::TopologyChanged
    );
    assert_eq!(owner.topology_epoch, 2);
    assert_eq!(owner.publication_generation, 2);
}

#[test]
fn parked_hardware_publication_is_dropped_after_an_equal_or_newer_policy_epoch() {
    assert!(!hardware_output_snapshot_is_stale(3, 2));
    assert!(hardware_output_snapshot_is_stale(3, 3));
    assert!(hardware_output_snapshot_is_stale(2, 3));
}

#[test]
fn a_second_policy_change_waits_until_post_commit_presentation_settles() {
    let mut owner = LiveOutputTopologyOwner::new_at_generation(
        vec![output(1, 1280)],
        vec![(OutputId::from_raw(1), 1)],
        1,
    )
    .unwrap();
    owner.begin_policy_change().unwrap();
    observe_policy_unmirrored(&mut owner, vec![output(1, 1920)], 2).unwrap();
    owner.mark_published(4, false).unwrap();

    assert!(owner.begin_policy_change().is_err());
    assert!(owner.observe_presentation(5));
    assert_eq!(owner.begin_policy_change(), Ok(true));
}

fn profile_value(key: &str, encoded: &str) -> sophia_config::DesktopProfileValue {
    sophia_config::DesktopProfileValue {
        key: key.to_owned(),
        encoded: encoded.to_owned(),
        provenance: sophia_config::DesktopValueProvenance {
            path: std::path::PathBuf::from("/etc/hagia/config.kdl"),
            ordinal: 1,
        },
    }
}

fn profile_generation(
    sections: &[(sophia_config::DesktopAuthority, &str)],
) -> sophia_config::DesktopProfileGeneration {
    sophia_config::DesktopProfileGeneration {
        generation: sophia_config::ConfigGeneration::INITIAL,
        digest: sophia_config::ConfigDigest::new([0; 32]),
        sources: Vec::new(),
        candidates: sections
            .iter()
            .map(|(authority, encoded)| {
                (
                    *authority,
                    sophia_config::DesktopAuthorityCandidate {
                        authority: *authority,
                        generation: sophia_config::ConfigGeneration::INITIAL,
                        digest: sophia_config::ConfigDigest::new([0; 32]),
                        values: vec![profile_value("value", encoded)],
                    },
                )
            })
            .collect(),
    }
}

#[test]
fn a_reload_that_left_the_displays_alone_builds_no_topology() {
    // The property that keeps a reload cheap. Editing a keybinding is the
    // common case by a wide margin, and it must not cost the operator a
    // modeset: the output section is untouched, so no candidate is ever built
    // and there is nothing that could blink a display.
    use sophia_config::DesktopAuthority;

    let before = profile_generation(&[
        (DesktopAuthority::Shortcut, "super+l"),
        (DesktopAuthority::Output, "DP-1 2560x1440@120"),
    ]);
    let after = profile_generation(&[
        (DesktopAuthority::Shortcut, "super+semicolon"),
        (DesktopAuthority::Output, "DP-1 2560x1440@120"),
    ]);

    let effects = desktop_profile_reload_effects(&before, &after);
    assert!(!effects.output_changed);
    assert!(effects.deferred.is_empty());
    assert!(!effects.policy_changed);
}

#[test]
fn a_reload_that_changed_the_displays_asks_for_a_topology_and_defers_nothing_else() {
    use sophia_config::DesktopAuthority;

    let before = profile_generation(&[(DesktopAuthority::Output, "DP-1 2560x1440@120")]);
    let after = profile_generation(&[(DesktopAuthority::Output, "DP-1 2560x1440@60")]);

    let effects = desktop_profile_reload_effects(&before, &after);
    assert!(effects.output_changed);
    // Output is the one non-policy authority a reload can act on, so it must
    // never also be reported as deferred -- an operator reading both lines
    // would not know which one happened.
    assert!(effects.deferred.is_empty());
}

#[test]
fn input_shell_and_broker_authorities_remain_deferred_by_a_reload() {
    // The honesty half: these were applied when the session started and a
    // reload cannot revisit them, so each one says so rather than letting a
    // changed key look effective.
    use sophia_config::DesktopAuthority;

    let sections = [
        DesktopAuthority::Shell,
        DesktopAuthority::Input,
        DesktopAuthority::Broker,
    ];
    let before = profile_generation(
        &sections
            .iter()
            .map(|authority| (*authority, "before"))
            .collect::<Vec<_>>(),
    );
    let after = profile_generation(
        &sections
            .iter()
            .map(|authority| (*authority, "after"))
            .collect::<Vec<_>>(),
    );

    let effects = desktop_profile_reload_effects(&before, &after);
    assert!(!effects.output_changed);
    assert_eq!(effects.deferred.len(), sections.len());
}

#[test]
fn startup_transaction_blocks_shell_before_execution_is_dispatched() {
    assert!(!owner_loop_shell_presentation_available(
        true, true, None, true
    ));
    for phase in [
        LiveOutputTopologyExecutionPhase::Applying,
        LiveOutputTopologyExecutionPhase::AwaitingFirstPresentation,
        LiveOutputTopologyExecutionPhase::Reconciling,
        LiveOutputTopologyExecutionPhase::RollingBack,
    ] {
        assert!(!owner_loop_shell_presentation_available(
            true,
            true,
            Some(phase),
            true
        ));
        assert!(!owner_loop_shell_presentation_available(
            true,
            true,
            Some(phase),
            false
        ));
    }
    assert!(owner_loop_shell_presentation_available(
        true, true, None, false
    ));
    assert!(!owner_loop_shell_presentation_available(
        false, true, None, false
    ));
    assert!(!owner_loop_shell_presentation_available(
        true, false, None, false
    ));
}
