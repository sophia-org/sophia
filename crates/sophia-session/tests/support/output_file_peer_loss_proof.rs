//! The proof requests real supervised-child termination; physical observations
//! are supplied. Native application and restoration require the attended gate.
use super::*;

fn start_child(rig: &mut Rig, protected: bool) {
    let mut spec = ProcessLaunchSpec::new(std::fs::canonicalize("/bin/sleep").unwrap())
        .arg("60")
        .process_group();
    if protected {
        spec = spec.protection_domain(
            sophia_runtime::ProtectionDomainSpec::bubblewrap([
                sophia_runtime::ProtectionDomainRole::OutputAuthority,
            ])
            .unwrap(),
        );
    }
    rig.supervisor().replace_launch_spec(spec).unwrap();
    rig.supervisor()
        .apply(sophia_runtime::SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::OutputAuthority,
            delay: Duration::ZERO,
        })
        .unwrap()
        .unwrap();
}

#[test]
fn output_peer_loss_proof_terminates_only_the_active_external_assignment() {
    exercise_termination(false);
}

#[test]
fn output_peer_loss_proof_observes_protected_supervisor_exit() {
    exercise_termination(true);
}

fn exercise_termination(protected: bool) {
    let mut rig = rig(if protected {
        "peer-loss-protected"
    } else {
        "peer-loss-proof"
    });
    start_child(&mut rig, protected);
    let mut peer = rig.peer();
    peer.negotiate();
    peer.propose(11, rig.apply());
    rig.admitted(11);
    let transaction = TransactionId::from_raw(11);
    assert_eq!(
        rig.fixture.wm.output_peer_transaction_epoch(transaction),
        Some(EPOCH)
    );
    rig.public().startup_output_transaction = Some(transaction);
    assert_eq!(
        rig.fixture.wm.output_peer_transaction_epoch(transaction),
        None
    );
    assert!(
        rig.fixture
            .wm
            .request_output_peer_proof_termination(transaction, EPOCH)
            .is_err()
    );
    rig.public().startup_output_transaction = None;
    rig.public().reload_output_transaction = Some(transaction);
    assert_eq!(
        rig.fixture.wm.output_peer_transaction_epoch(transaction),
        None
    );
    rig.public().reload_output_transaction = None;
    assert!(
        rig.fixture
            .wm
            .request_output_peer_proof_termination(transaction, EPOCH + 1)
            .is_err()
    );
    let heads = rig.dispatch(11);
    rig.apply_heads(11, &heads);
    peer.quiet();
    assert!(rig.public().output_cancel_requested.is_none());
    rig.fixture
        .wm
        .request_output_peer_proof_termination(transaction, EPOCH)
        .unwrap();
    // A termination request is not a policy cancellation. Only polling the
    // actual exit and the worker's resulting disconnect may create the debt.
    assert!(rig.public().output_cancel_requested.is_none());
    rig.cancellation_requested(11);
    assert!(rig.supervisor().child_id().is_none());
    let observation = rig.fixture.wm.output_peer_loss_observation().unwrap();
    assert!(observation.terminated && observation.disconnected && !observation.failed);
    rig.assert_debt(11);
    assert_eq!(
        rig.cancel_execution(
            11,
            Execution::AwaitingFirstPresentation,
            sophia_backend_live::LiveProductionNativeTopologyPreparationPhase::RollingBack,
        ),
        Execution::RollingBack
    );
    rig.public()
        .observe_output_topology_rolled_back(transaction, &heads)
        .unwrap();
    rig.assert_preserved();
    peer.close();
}

#[test]
fn output_peer_loss_supervision_continues_while_cancellation_holds_events() {
    let mut rig = rig("peer-loss-debt-reap");
    start_child(&mut rig, false);
    let mut peer = rig.peer();
    peer.negotiate();
    peer.propose(11, rig.apply());
    rig.admitted(11);
    rig.dispatch(11);
    peer.close();
    rig.cancellation_requested(11);
    assert!(rig.supervisor().child_id().is_some());
    rig.supervisor().request_termination().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while rig.supervisor().child_id().is_some() {
        assert!(Instant::now() < deadline, "rollback debt prevented reaping");
        rig.fixture.wm.poll_output_authority().unwrap();
        std::thread::yield_now();
    }
    rig.assert_debt(11);
    rig.public()
        .reject_output_topology_effect(
            TransactionId::from_raw(11),
            OutputTopologyTransactionFailure::Stale,
        )
        .unwrap();
    rig.assert_preserved();
}
