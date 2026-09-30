//! A profile-owned startup proposal has no transport ticket. Losing an output
//! observer cannot cancel it or make its local settlement target that peer.
//! Admission mirrors session_start; physical completion remains supplied.
use super::*;
use crate::live_output_authority::LiveOutputAuthorityAdmission;

const STARTUP: u64 = u64::MAX;

#[test]
fn startup_rejection_stays_local_with_a_connected_observer() {
    let mut rig = startup("startup-connected");
    let mut observer = rig.peer();
    let (epoch, original, _) = observer.negotiate();
    assert_eq!(epoch, EPOCH);
    rig.fixture.wm.poll_output_authority().unwrap();
    rig.dispatch(STARTUP);
    rig.public()
        .reject_output_topology_effect(
            TransactionId::from_raw(STARTUP),
            OutputTopologyTransactionFailure::Stale,
        )
        .unwrap();
    // A mistaken transport Settle names an unknown transaction on this live
    // epoch. It must not reach the observer or fail the file service.
    observer.quiet();
    rig.fixture.wm.poll_output_authority().unwrap();
    assert!(rig.public().output_service.is_some());
    assert!(rig.public().startup_output_transaction.is_none());
    assert!(rig.public().output_pending_connection_epoch.is_none());
    assert!(rig.public().output_cancel_requested.is_none());
    assert!(!rig.public().output_candidate_active());
    assert_eq!(rig.public().published_output_snapshot(), Some(original));
    assert_eq!(rig.authority().connection_epoch(), EPOCH);
    // Reuse the same connection, rather than relying on a new listener epoch.
    observer.propose(11, rig.apply());
    rig.admitted(11);
    observer.close();
}

fn startup(label: &str) -> Rig {
    let mut rig = rig(label);
    let candidate = rig.apply();
    let capability = rig.capability.clone();
    let public = rig.public();
    let admission = public
        .output_authority
        .as_mut()
        .unwrap()
        .admit(
            TransactionId::from_raw(STARTUP),
            &OutputV1Proposal {
                connection_epoch: EPOCH,
                candidate,
            },
            &[capability],
        )
        .unwrap();
    assert!(matches!(admission, LiveOutputAuthorityAdmission::Prepared));
    public.startup_output_transaction = Some(TransactionId::from_raw(STARTUP));
    assert!(public.output_topology_effect_pending());
    rig
}

fn assert_survives_disconnect(rig: &mut Rig) {
    rig.until("startup observer disconnect", |public| {
        public.output_pending_connection_epoch == Some(EPOCH + 1)
    });
    assert_eq!(
        rig.authority().active_transaction(),
        Some(TransactionId::from_raw(STARTUP))
    );
    assert_eq!(rig.authority().connection_epoch(), EPOCH);
    assert_eq!(
        rig.public().startup_output_transaction,
        Some(TransactionId::from_raw(STARTUP))
    );
    assert!(rig.public().output_cancel_requested.is_none());
    assert_eq!(
        rig.public().published_output_snapshot(),
        Some(rig.snapshot.clone())
    );
    assert!(rig.public().output_service.is_some());
}

#[test]
fn startup_disconnect_before_dispatch_preserves_effect_and_rejects_locally() {
    let mut rig = startup("startup-before-dispatch");
    let mut observer = rig.peer();
    let (_, original, qid) = observer.negotiate();
    observer.close();
    assert_survives_disconnect(&mut rig);
    assert!(!rig.public().output_effect_dispatched);
    assert!(rig.public().output_topology_effect_pending());

    // The original profile effect remains available after the peer is gone.
    rig.dispatch(STARTUP);
    rig.public()
        .reject_output_topology_effect(
            TransactionId::from_raw(STARTUP),
            OutputTopologyTransactionFailure::Stale,
        )
        .unwrap();
    rig.assert_preserved();
    assert!(rig.public().startup_output_transaction.is_none());
    assert!(rig.public().output_pending_connection_epoch.is_none());
    let mut replacement = rig.replacement(&original, qid);
    replacement.propose(11, rig.apply());
    rig.admitted(11);
    replacement.close();
}

#[test]
fn startup_disconnect_after_apply_preserves_candidate_and_commits_locally() {
    let mut rig = startup("startup-after-apply");
    let mut observer = rig.peer();
    let (_, _, qid) = observer.negotiate();
    let heads = rig.dispatch(STARTUP);
    rig.apply_heads(STARTUP, &heads);
    observer.close();
    assert_survives_disconnect(&mut rig);
    assert!(rig.public().output_effect_dispatched);
    assert_eq!(
        rig.authority().active_phase(),
        Some(OutputTopologyTransactionPhase::AwaitingFirstPresentation)
    );
    let outputs = rig
        .authority()
        .active_resolved()
        .unwrap()
        .outputs
        .iter()
        .map(|output| output.id)
        .collect::<Vec<_>>();
    let committed = rig
        .public()
        .observe_output_topology_first_presented(TransactionId::from_raw(STARTUP), &outputs)
        .unwrap()
        .expect("startup committed snapshot");
    assert_eq!(committed.topology_epoch, TOPOLOGY_EPOCH + 1);
    assert_eq!(committed.heads[0].current_mode, Some(rig.alternate));
    assert_eq!(rig.authority().connection_epoch(), EPOCH + 1);
    assert!(!rig.public().output_candidate_active());
    assert!(rig.public().startup_output_transaction.is_none());
    assert!(rig.public().output_pending_connection_epoch.is_none());
    assert!(rig.public().output_cancel_requested.is_none());
    assert!(!rig.public().output_effect_dispatched);
    assert!(rig.public().take_output_topology_effect().is_none());
    assert!(rig.public().output_service.is_some());
    assert_eq!(
        rig.public().published_output_snapshot(),
        Some(committed.clone())
    );
    // No Outcome exists for the profile transaction on the replacement epoch.
    rig.replacement(&committed, qid).close();
}
