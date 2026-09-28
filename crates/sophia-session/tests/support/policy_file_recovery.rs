//! Independent C SDK recovery over the real protected WM file connection.
//! Layout failure is supplied to the production settlement owner: this proves
//! cycle rearming and client recovery, not a real resize deadline or scanout.
use super::*;
use crate::live_session::c_sdk_fixture_process as process;
use std::os::unix::fs::PermissionsExt;

#[test]
fn protected_c_sdk_recovers_after_stale_and_timed_out_projections() {
    let scratch = process::Scratch::new();
    let root = &scratch.0;
    let executable = process::compile(
        root,
        &["nine_p", "wm_files", "wm_session"],
        "live_control_peer.c",
    );
    let profile = root.join("desktop.kdl");
    std::fs::write(&profile, "schema 1\n").unwrap();
    std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut config = PersistentXtermSessionConfig::from_args(&[
        format!("--desktop-profile={}", profile.display()),
        format!("--wm-process={}", executable.display()),
        "--wm-interface=sophia_wm_v1".into(),
        "--wm-transport=9p2000.L".into(),
        "--wm-process-arg=--recovery".into(),
    ])
    .unwrap();
    config.wm_socket_path = root.join("wm.sock");
    let prepared = LiveWmSession::prepare_public_launch(&mut config).unwrap();
    let started = LiveWmSession::activate_public_launch(&mut config, prepared).unwrap();
    let output = sophia_engine::HeadlessOutput::deterministic();
    let mut wm = LiveWmSession::from_config(&config, &[output], started, None)
        .unwrap()
        .unwrap();
    let pid = rustix::process::Pid::from_raw(wm.supervisor.child_id().unwrap() as i32).unwrap();
    let mut layout = PersistentLiveLayout::default();
    let mut request_ids = Vec::new();
    let mut transactions = Vec::new();
    for index in 0..3 {
        let deadline = Instant::now() + Duration::from_secs(5);
        let proposal = loop {
            let proposal = wm.poll_request(&mut layout, output, true).unwrap();
            wm.settle_desktop_reload(&mut config, true).unwrap();
            if let Some(proposal) = proposal {
                break proposal;
            }
            assert!(Instant::now() < deadline, "fresh recovery cycle {index}");
            std::thread::sleep(Duration::from_millis(1));
        };
        let identity = proposal.policy_settlement.unwrap();
        assert_eq!(identity.connection_epoch, 1);
        assert!(
            request_ids
                .last()
                .is_none_or(|old| *old < identity.request_id)
        );
        assert!(!transactions.contains(&identity.transaction));
        request_ids.push(identity.request_id);
        transactions.push(identity.transaction);
        assert_eq!(wm.committed, 0);
        if index == 2 {
            let result = layout.commit_proposal(proposal);
            wm.apply_commit_result(result, None, output.id).unwrap();
            assert_eq!(wm.committed, 1);
        } else {
            if index == 0 {
                // Advance the canonical scene while a proposal is staged.
                let public = wm.public.as_mut().unwrap();
                let mut scene = public.reducer.scene().clone();
                scene.generation += 1;
                public.reducer.observe_scene(scene).unwrap();
            }
            let result = LiveWmCommitResult {
                update: WmTransactionUpdate {
                    commit: TransactionCommit {
                        transaction: proposal.transaction,
                        outcome: TransactionOutcome::TimedOut,
                        applied_surfaces: vec![],
                    },
                },
                source: proposal.source,
                policy_settlement: proposal.policy_settlement,
            };
            wm.apply_commit_result(result, None, output.id).unwrap();
            assert_eq!(wm.committed, 0);
            assert_eq!(wm.topology_policy_commit_serial(), 0);
        }
    }
    // Zero exit proves the peer decoded all three exact terminal outcomes.
    // Observe only; the production supervisor retains reaping ownership.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = rustix::process::waitid(
            rustix::process::WaitId::Pid(pid),
            rustix::process::WaitIdOptions::EXITED
                | rustix::process::WaitIdOptions::NOHANG
                | rustix::process::WaitIdOptions::NOWAIT,
        )
        .unwrap()
        {
            assert_eq!(status.exit_status(), Some(0), "C recovery peer: {status:?}");
            break;
        }
        assert!(Instant::now() < deadline, "C peer receives final outcome");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(wm.restarts, 0);
    assert_eq!(wm.public.as_ref().unwrap().connection_epoch, 1);
}
