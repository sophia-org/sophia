// The owner's obligation to re-offer a cycle after a recoverable rejection.
//
// A physical run stranded here: the owner rejected a response as stale, queued
// nothing, and went idle. The client was waiting for the snapshot that rejection
// implies, hit its socket deadline, exited, and the resulting restarts exhausted
// the supervisor budget and killed the session.

use super::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::live_session::{
    LivePublicPolicyCause, LiveWmProposalSource, LiveWmRequestAdmission,
    consume_public_launch_classification, enqueue_public_policy_cause,
    materialize_public_dirty_cause, public_launch_classification_snapshot,
    public_policy_rearm_after_outcome, public_policy_snapshot_focus, resolve_output_policy_key,
};

fn relayout_cause(outputs: &[u64]) -> LivePublicPolicyCause {
    LivePublicPolicyCause {
        source: LiveWmProposalSource::Relayout,
        cause: sophia_protocol::PolicyRequestCause::SceneChanged,
        affected_outputs: outputs.iter().copied().map(OutputId::from_raw).collect(),
    }
}

fn output_set(outputs: &[u64]) -> BTreeSet<OutputId> {
    outputs.iter().copied().map(OutputId::from_raw).collect()
}

#[test]
fn launch_classification_survives_retry_and_is_consumed_only_by_manage_commit() {
    let surface = SurfaceId::new(7, 1);
    let mut classifications = BTreeMap::from([(surface, 2)]);
    let source = Some(LiveWmProposalSource::Manage(surface));
    for outcome in [
        sophia_protocol::PolicyProjectionOutcome::RejectedStale,
        sophia_protocol::PolicyProjectionOutcome::RejectedInvalid,
        sophia_protocol::PolicyProjectionOutcome::TimedOut,
        sophia_protocol::PolicyProjectionOutcome::Disconnected,
    ] {
        assert_eq!(
            consume_public_launch_classification(&mut classifications, source, outcome),
            None
        );
        assert_eq!(classifications.get(&surface), Some(&2));
    }
    assert_eq!(
        consume_public_launch_classification(
            &mut classifications,
            Some(LiveWmProposalSource::Relayout),
            sophia_protocol::PolicyProjectionOutcome::Committed,
        ),
        None
    );
    assert_eq!(classifications.get(&surface), Some(&2));
    assert_eq!(
        consume_public_launch_classification(
            &mut classifications,
            source,
            sophia_protocol::PolicyProjectionOutcome::Committed,
        ),
        Some((surface, 2))
    );
    assert!(classifications.is_empty());
}

#[test]
fn launch_classification_snapshot_excludes_withdrawn_surfaces() {
    let live = SurfaceId::new(7, 1);
    let withdrawn = SurfaceId::new(8, 1);
    let scene = sophia_protocol::PolicySceneSnapshot {
        generation: 1,
        active_output: OutputId::from_raw(1),
        outputs: vec![sophia_protocol::PolicyOutputSnapshot {
            policy_key: None,
            output: OutputId::from_raw(1),
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
        }],
        surfaces: vec![sophia_protocol::PolicySurfaceSnapshot {
            surface: live,
            generation: 1,
            current_output: None,
            kind: sophia_protocol::PolicySurfaceKind::Toplevel,
            capabilities: sophia_protocol::LayoutNodeCapabilities::STANDARD_TOPLEVEL,
            constraints: sophia_protocol::SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            exact_size: None,
            requested_state: sophia_protocol::PolicyPresentationState::default(),
            current_state: sophia_protocol::PolicyPresentationState::default(),
            transient_owner: None,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 50,
                height: 50,
            },
        }],
        session_operations: Vec::new(),
    };
    assert_eq!(
        public_launch_classification_snapshot(&BTreeMap::from([(live, 2), (withdrawn, 3)]), &scene,),
        vec![sophia_protocol::PolicySurfaceClassification {
            surface: live,
            classification: 2,
        }]
    );
}

#[test]
fn snapshot_focus_requires_a_live_focusable_surface_on_the_same_output() {
    let output = OutputId::from_raw(1);
    let focus = SurfaceId::new(7, 1);
    let mut surface = sophia_protocol::PolicySurfaceSnapshot {
        surface: focus,
        generation: 1,
        current_output: Some(output),
        kind: sophia_protocol::PolicySurfaceKind::Toplevel,
        capabilities: sophia_protocol::LayoutNodeCapabilities::STANDARD_TOPLEVEL,
        constraints: sophia_protocol::SurfaceConstraints {
            min_size: None,
            max_size: None,
        },
        exact_size: None,
        requested_state: sophia_protocol::PolicyPresentationState::default(),
        current_state: sophia_protocol::PolicyPresentationState::default(),
        transient_owner: None,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 50,
            height: 50,
        },
    };
    assert_eq!(
        public_policy_snapshot_focus(output, Some(focus), &[surface]),
        Some(focus)
    );
    assert_eq!(public_policy_snapshot_focus(output, Some(focus), &[]), None);

    surface.current_output = Some(OutputId::from_raw(2));
    assert_eq!(
        public_policy_snapshot_focus(output, Some(focus), &[surface]),
        None
    );
    surface.current_output = Some(output);
    surface.capabilities.focusable = false;
    assert_eq!(
        public_policy_snapshot_focus(output, Some(focus), &[surface]),
        None
    );
    surface.capabilities.focusable = true;
    surface.current_state.minimized = true;
    assert_eq!(
        public_policy_snapshot_focus(output, Some(focus), &[surface]),
        None
    );
}

#[test]
fn the_owner_rearms_for_exactly_the_outcomes_a_stateless_client_retries() {
    // The two halves of one contract. A client that recovers by awaiting a
    // fresh snapshot depends on the owner deciding to send one, so these tables
    // must not drift apart.
    for outcome in [
        sophia_protocol::PolicyProjectionOutcome::Committed,
        sophia_protocol::PolicyProjectionOutcome::RejectedStale,
        sophia_protocol::PolicyProjectionOutcome::RejectedInvalid,
        sophia_protocol::PolicyProjectionOutcome::TimedOut,
        sophia_protocol::PolicyProjectionOutcome::Disconnected,
    ] {
        let client_retries = matches!(
            sophia_wm_demo::stateless_reference_projection_decision(outcome),
            sophia_wm_demo::StatelessReferenceProjectionDecision::RetryFreshSnapshot
        );
        assert_eq!(
            public_policy_rearm_after_outcome(outcome),
            client_retries,
            "owner and client disagree about recovering from {outcome:?}"
        );
    }
}

#[test]
fn an_invalid_rejection_does_not_rearm() {
    // The scene did not move, so re-offering the cycle would spin on the same
    // faulty proposal rather than converging.
    assert!(!public_policy_rearm_after_outcome(
        sophia_protocol::PolicyProjectionOutcome::RejectedInvalid
    ));
    assert!(!public_policy_rearm_after_outcome(
        sophia_protocol::PolicyProjectionOutcome::Disconnected
    ));
}

#[test]
fn dirty_outputs_merge_into_one_queued_relayout() {
    let mut queue = VecDeque::from(vec![relayout_cause(&[1])]);
    let mut pending = output_set(&[2, 3]);

    materialize_public_dirty_cause(&mut queue, &mut pending, None);

    assert_eq!(queue.len(), 1, "merging must not grow the queue");
    assert_eq!(
        queue[0].affected_outputs,
        vec![
            OutputId::from_raw(1),
            OutputId::from_raw(2),
            OutputId::from_raw(3)
        ]
    );
    assert!(pending.is_empty());
}

#[test]
fn dirty_outputs_defer_while_a_relayout_is_in_flight() {
    let mut queue = VecDeque::new();
    let mut pending = output_set(&[2]);

    materialize_public_dirty_cause(
        &mut queue,
        &mut pending,
        Some(LiveWmProposalSource::Relayout),
    );

    assert!(queue.is_empty());
    assert_eq!(pending, output_set(&[2]), "deferred outputs are not lost");
}

#[test]
fn dirty_outputs_push_one_relayout_when_none_is_queued() {
    let mut queue = VecDeque::new();
    let mut pending = output_set(&[4, 5]);

    materialize_public_dirty_cause(&mut queue, &mut pending, None);

    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].source, LiveWmProposalSource::Relayout);
    assert_eq!(
        queue[0].cause,
        sophia_protocol::PolicyRequestCause::SceneChanged
    );
    assert!(pending.is_empty());
}

#[test]
fn a_repeated_rearm_never_grows_the_queue_beyond_one_relayout() {
    // A stale storm re-arms on every rejection. The queue must stay bounded.
    let mut queue = VecDeque::new();
    for _ in 0..8 {
        let mut pending = output_set(&[1, 2]);
        materialize_public_dirty_cause(&mut queue, &mut pending, None);
    }
    assert_eq!(queue.len(), 1);
}

#[test]
fn a_duplicate_relayout_admission_merges_the_replacement_output_set() {
    // A topology change enqueues a relayout naming the new live outputs. When
    // one is already queued the admission reports Duplicate, and dropping the
    // replacement would leave the queued cause naming an output that no longer
    // exists — which fails when the owner tries to issue it.
    let mut queue = VecDeque::from(vec![relayout_cause(&[1, 9])]);
    let admission = enqueue_public_policy_cause(&mut queue, None, false, relayout_cause(&[1, 2]));
    assert_eq!(admission, LiveWmRequestAdmission::Duplicate);

    let mut replacement = output_set(&[1, 2]);
    materialize_public_dirty_cause(&mut queue, &mut replacement, None);

    assert_eq!(queue.len(), 1);
    assert!(
        queue[0].affected_outputs.contains(&OutputId::from_raw(2)),
        "the replacement live output must survive a duplicate admission"
    );
}

#[test]
fn a_recovery_request_naming_a_withdrawn_surface_is_filtered_before_it_is_issued() {
    // `begin_recovery` fails the whole call when any requested surface has no
    // committed extent, and that is correct: there is nothing to configure it
    // back to. The caller therefore has to filter, and for a long time it did
    // so only for its fixed set. A physical run ended the session over one
    // surface that had been withdrawn while its resize was outstanding.
    let live = SurfaceId::new(21, 1);
    let withdrawn = SurfaceId::new(22, 1);
    let committed = Size {
        width: 500,
        height: 400,
    };
    let mut epochs = sophia_engine::LayoutEpochCoordinator::default();
    epochs.record_committed(live, committed);

    // Unfiltered, the withdrawn surface takes the whole recovery down.
    let mut unfiltered = sophia_engine::LayoutEpochCoordinator::default();
    unfiltered.record_committed(live, committed);
    assert!(
        unfiltered
            .begin_recovery(
                [
                    (
                        live,
                        Size {
                            width: 900,
                            height: 700
                        }
                    ),
                    (
                        withdrawn,
                        Size {
                            width: 900,
                            height: 700
                        }
                    ),
                ],
                [],
            )
            .is_err()
    );

    // Filtered the way the caller now filters, the recoverable surface still
    // recovers.
    let requests = [
        (
            live,
            Size {
                width: 900,
                height: 700,
            },
        ),
        (
            withdrawn,
            Size {
                width: 900,
                height: 700,
            },
        ),
    ]
    .into_iter()
    .filter(|(surface, _)| epochs.safe_size(*surface).is_some())
    .collect::<Vec<_>>();
    let configures = epochs
        .begin_recovery(requests, [])
        .expect("a filtered recovery must not fail on a withdrawn surface");
    assert_eq!(configures.len(), 1);
    assert_eq!(configures[0].surface, live);
    assert_eq!(configures[0].size, committed);
}

/// Closing an application with more surfaces than the queue is deep must not
/// end the session.
///
/// Withdrawals used to name their surface as the source. The queue
/// deduplicates by source, so they never coalesced with one another, and
/// quitting a browser holding seventeen surfaces overflowed a sixteen-deep
/// queue and killed the session with "WM surface_removed request exceeded the
/// owner queue capacity". They are relayouts now, and a relayout already
/// pending is the same relayout.
#[test]
fn withdrawing_many_surfaces_at_once_coalesces_into_one_relayout() {
    let mut queue = VecDeque::new();
    let outputs = &[1u64];

    // Comfortably more surfaces than WM_OWNER_REQUEST_CAPACITY.
    let mut admissions = Vec::new();
    for _ in 0..64 {
        admissions.push(enqueue_public_policy_cause(
            &mut queue,
            None,
            false,
            relayout_cause(outputs),
        ));
    }

    assert_eq!(admissions[0], LiveWmRequestAdmission::Admitted);
    assert!(
        admissions[1..]
            .iter()
            .all(|admission| *admission == LiveWmRequestAdmission::Duplicate),
        "every later withdrawal must fold into the pending relayout"
    );
    assert!(
        !admissions.contains(&LiveWmRequestAdmission::RejectedCapacity),
        "a capacity rejection here is the session ending"
    );
    assert_eq!(queue.len(), 1, "the whole burst is one relayout");
}

#[test]
fn configured_output_keys_resolve_connectors_not_output_order() {
    use sophia_backend_live::{
        LibdrmNativeOutputCapability, LibdrmNativeOutputTiming,
        LibdrmNativeVrrPropertyDiscoveryStatus,
    };
    let capability = |output, connector: &str| {
        let timing = LibdrmNativeOutputTiming::new(1000, 700, 60_000);
        LibdrmNativeOutputCapability::new(
            OutputId::from_raw(output),
            output as u32,
            connector,
            [timing],
            Some(timing),
            timing,
            LibdrmNativeVrrPropertyDiscoveryStatus::Discovered,
        )
        .unwrap()
    };
    let keys = [("DP-1".to_owned(), 17), ("DP-2".to_owned(), 29)]
        .into_iter()
        .collect();
    let reversed = [capability(1, "DP-2"), capability(2, "DP-1")];
    assert_eq!(
        resolve_output_policy_key(OutputId::from_raw(1), &keys, &reversed).unwrap(),
        Some(29)
    );
    assert_eq!(
        resolve_output_policy_key(OutputId::from_raw(2), &keys, &reversed).unwrap(),
        Some(17)
    );
    assert_eq!(
        resolve_output_policy_key(OutputId::from_raw(3), &keys, &reversed).unwrap(),
        None
    );
    let ambiguous = [capability(1, "DP-1"), capability(1, "DP-2")];
    assert!(resolve_output_policy_key(OutputId::from_raw(1), &keys, &ambiguous).is_err());
}
