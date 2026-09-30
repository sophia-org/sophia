//! The generic C peer uses Session's actual protected launch and checked
//! admission. Native preparation, apply and presentation are supplied here.
//! No KMS access or physical restoration is implied by these observations.
use super::*;
use std::io::{Read, Write};

#[test]
#[ignore = "requires the prepared generic SOPHIA_OUTPUT_NATIVE_PROOF_PEER"]
fn native_proof_peer_uses_session_supervision_and_owner_settlement() {
    let peer = PathBuf::from(
        std::env::var_os("SOPHIA_OUTPUT_NATIVE_PROOF_PEER")
            .expect("prepare the generic C proof peer first"),
    )
    .canonicalize()
    .unwrap();
    for stage in [
        "validate",
        "reject",
        "commit-restore",
        "apply-await-termination",
    ] {
        exercise(&peer, stage);
    }
}

fn exercise(peer: &Path, stage: &str) {
    let mut rig = rig(&format!("native-session-{stage}"));
    // Replace the numeric test-owner service with the production launch recipe.
    rig.public().output_service.take();
    let directory = rig.socket.parent().unwrap().to_owned();
    let baseline = rig.snapshot.clone();
    let initial = baseline.heads[0].current_mode.unwrap();
    let group = &baseline.groups[0];
    let head = baseline.heads[0].head.raw();
    let mapping = match group.members[0].mapping {
        sophia_protocol::OutputHeadMapping::Fit => "fit",
        sophia_protocol::OutputHeadMapping::Cover => "cover",
        sophia_protocol::OutputHeadMapping::Exact => "exact",
    };
    let config = &mut rig.fixture.source.config;
    config.output_process = Some(peer.to_str().unwrap().to_owned());
    config.output_process_args = vec![
        format!("--stage={stage}"),
        "--deadline-ms=5000".into(),
        format!("--a-topology-epoch={}", baseline.topology_epoch),
        format!("--a-heads={head}:{}:normal:disabled", initial.raw()),
        format!(
            "--a-groups={}@{},{},{}x{}={head}/{mapping}",
            group.output.raw(),
            group.logical.x,
            group.logical.y,
            group.logical.width,
            group.logical.height
        ),
        "--a-primary=0".into(),
    ];
    if stage != "reject" {
        config.output_process_args.extend([
            format!("--b-heads={head}:{}:normal:disabled", rig.alternate.raw()),
            format!(
                "--b-groups={}@{},{},{}x{}={head}/{mapping}",
                group.output.raw(),
                group.logical.x,
                group.logical.y,
                group.logical.width,
                group.logical.height
            ),
            "--b-primary=0".into(),
        ]);
    }
    let prepared = PreparedOutputTransport::bind(config, &directory).unwrap();
    let PreparedOutputTransport::Files { supervisor, .. } = &prepared else {
        panic!("file role")
    };
    assert_eq!(
        supervisor.protection_evidence().unwrap().roles,
        [sophia_runtime::ProtectionDomainRole::OutputAuthority]
            .into_iter()
            .collect()
    );
    let peer_pid = supervisor.peer_id().unwrap();
    rig.public().output_authority =
        Some(LiveOutputAuthorityOwner::new(1, baseline.clone()).unwrap());
    rig.public().output_service = Some(prepared.start(baseline.clone()).unwrap());
    let killing = stage == "apply-await-termination";
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut effects = 0;
    let mut rollback = None;
    loop {
        assert!(Instant::now() < deadline, "Session stage {stage} timed out");
        rig.fixture.wm.poll_output_authority().unwrap();
        let exit = rig.supervisor().exit_status();
        assert!(
            rig.public().output_service.is_some(),
            "service failed in {stage}"
        );
        if let Some(effect) = rig.fixture.wm.take_output_topology_effect() {
            effects += 1;
            assert!(
                stage == "commit-restore" || killing,
                "unexpected physical effect in {stage}"
            );
            assert_eq!(
                effect.candidate_snapshot.heads[0].current_mode,
                Some(if effects == 1 { rig.alternate } else { initial })
            );
            let transaction = effect.transaction;
            let heads = effect.resolved.affected_heads().collect::<Vec<_>>();
            rig.public()
                .begin_output_topology_apply(transaction, &heads)
                .unwrap();
            rig.public()
                .observe_output_topology_applied(transaction, &heads)
                .unwrap();
            if killing {
                assert_eq!(effects, 1);
                rig.fixture
                    .wm
                    .request_output_peer_proof_termination(transaction, 1)
                    .unwrap();
                assert!(rig.public().output_cancel_requested.is_none());
                rollback = Some((transaction, heads));
            } else {
                let outputs = effect
                    .resolved
                    .outputs
                    .iter()
                    .map(|output| output.id)
                    .collect::<Vec<_>>();
                let snapshot = rig
                    .public()
                    .observe_output_topology_first_presented(transaction, &outputs)
                    .unwrap()
                    .expect("supplied first presentation commits");
                assert_eq!(snapshot.topology_epoch, TOPOLOGY_EPOCH + effects);
            }
        }
        if let Some((transaction, heads)) = rollback.as_ref()
            && rig
                .fixture
                .wm
                .output_topology_cancellation_reason(*transaction)
                .is_some()
        {
            assert_eq!(
                rig.public().published_output_snapshot(),
                Some(baseline.clone())
            );
            assert_eq!(
                rig.cancel_execution(
                    transaction.raw(),
                    Execution::AwaitingFirstPresentation,
                    sophia_backend_live::LiveProductionNativeTopologyPreparationPhase::RollingBack
                ),
                Execution::RollingBack
            );
            rig.public()
                .observe_output_topology_rolled_back(*transaction, heads)
                .unwrap();
            rollback = None;
        }
        if killing {
            if let Some(observed) = rig.fixture.wm.output_peer_loss_observation()
                && observed.terminated
                && observed.disconnected
            {
                assert!(!observed.failed);
                assert_eq!(observed.peer, peer_pid);
                assert!(!rig.fixture.wm.output_peer_supervisor_running());
                assert!(!rig.public().output_candidate_active());
                break;
            }
        } else if exit.is_some() && !rig.public().output_candidate_active() {
            assert!(exit.unwrap().success(), "peer failed in {stage}: {exit:?}");
            break;
        }
        std::thread::yield_now();
    }
    assert_eq!(
        effects,
        if killing {
            1
        } else if stage == "commit-restore" {
            2
        } else {
            0
        }
    );
    let mut expected = baseline;
    if stage == "commit-restore" {
        expected.topology_epoch += 2;
        expected.heads[0].generation += 2;
        expected.groups[0].generation += 2;
    }
    assert_eq!(rig.public().published_output_snapshot(), Some(expected));
    assert!(rig.public().output_cancel_requested.is_none());
    assert!(rig.public().take_output_topology_effect().is_none());
    // A real protocol request, not just connect(), discriminates a paused
    // listener from an admitted peer that has not negotiated yet.
    let mut probe = std::os::unix::net::UnixStream::connect(&rig.socket).unwrap();
    probe
        .set_read_timeout(Some(Duration::from_millis(150)))
        .unwrap();
    probe
        .set_write_timeout(Some(Duration::from_millis(150)))
        .unwrap();
    let mut version = 21u32.to_le_bytes().to_vec();
    version.push(100);
    version.extend(u16::MAX.to_le_bytes());
    version.extend(65536u32.to_le_bytes());
    version.extend(8u16.to_le_bytes());
    version.extend(b"9P2000.L");
    probe.write_all(&version).unwrap();
    assert!(matches!(probe.read(&mut [0; 1]), Err(error)
        if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)));
    assert!(!rig.fixture.wm.output_peer_supervisor_running());
}
