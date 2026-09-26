use super::*;
use sophia_protocol::{ControlCommand, ControlOutcome, ControlOwner};
use sophia_runtime::{ControlClient, ControlService};
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

// The supervisor executes this test binary inside its ordinary policy domain.
#[test]
fn policy_fixture_process() {
    let Ok(socket) = std::env::var(sophia_runtime::SOPHIA_WM_SOCKET_ENV) else {
        return;
    };
    assert!(std::env::var_os(sophia_runtime::SOPHIA_CONTROL_SOCKET_ENV).is_none());
    let mut client =
        sophia_wm_demo::PolicyV1Client::connect(socket, Duration::from_secs(3)).unwrap();
    client
        .activate_profile_and_configure_with(
            vec![sophia_protocol::PolicyActionRegistration {
                action: WmActionId::from_raw(1),
                name: "focus-next".into(),
                session_operation_slot: None,
            }],
            sophia_protocol::WmChromePolicy::default(),
        )
        .unwrap();
    loop {
        let scene = match client.receive_snapshot() {
            Ok(scene) => scene,
            Err(_) => return,
        };
        let request = client.receive_projection_request().unwrap();
        let proposal = client.tile_once(&scene.scene, &request).unwrap();
        client.send_projection(&proposal).unwrap();
        client.receive_projection_outcome(&proposal).unwrap();
    }
}

#[test]
#[ignore = "runs a supervised policy process in bubblewrap; tools/check_control_protocol.sh --live-owner"]
fn real_owner_commits_actions_and_confirms_replacement_commit() {
    let _ = crate::install_session_output(crate::SessionOutput::new(
        |line| eprintln!("{line}"),
        |line| eprintln!("{line}"),
    ));
    let root = std::env::temp_dir().join(format!("sc-owner-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let profile = root.join("desktop.kdl");
    std::fs::write(&profile, "schema 1\nsession { control \"host-admin\"; }\n").unwrap();
    std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o600)).unwrap();
    let executable = std::env::current_exe().unwrap();
    let mut config = PersistentXtermSessionConfig::from_args(&[
        format!("--desktop-profile={}", profile.display()),
        format!("--wm-process={}", executable.display()),
        "--wm-interface=sophia_wm_v1".into(),
        "--wm-process-arg=--exact".into(),
        "--wm-process-arg=live_session::live_control_tests::policy_fixture_process".into(),
        "--wm-process-arg=--nocapture".into(),
    ])
    .unwrap();
    config.wm_socket_path = root.join("wm.sock");
    let prepared = LiveWmSession::prepare_public_launch(&mut config).unwrap();
    let started = LiveWmSession::activate_public_launch(&mut config, prepared).unwrap();
    let output = sophia_engine::HeadlessOutput::deterministic();
    let mut wm = LiveWmSession::from_config(&config, &[output], started, None)
        .unwrap()
        .unwrap();
    let mut layout = PersistentLiveLayout::default();
    let mut scripting = LiveControlState {
        service: Some(ControlService::bind(&root).unwrap()),
        catalog: Arc::new(sophia_protocol::ControlCatalog {
            generation: 1,
            commands: Vec::new(),
        }),
        signature: None,
        next: None,
        restarting: None,
        reloading: None,
        requests: SessionControlRequests::default(),
        published: false,
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while wm.committed == 0 {
        pump(&mut wm, &mut layout, output);
        assert!(Instant::now() < deadline, "initial policy commit");
    }
    scripting.service(Some(&mut wm), &layout, output, false);
    let socket = scripting
        .service
        .as_ref()
        .unwrap()
        .socket_path()
        .to_path_buf();
    // Revision 2 advertises the three session operations beside the action.
    let names = scripting
        .catalog
        .commands
        .iter()
        .map(|c| c.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["focus-next", "logout", "reload-profile", "restart-wm"]
    );
    // A revision-1 connection still sees only restart-wm among them.
    let path = socket.clone();
    let revision_one = std::thread::spawn(move || {
        let mut client = ControlClient::connect_revisions(&path, 1, 1).unwrap();
        (client.revision(), client.commands().unwrap())
    });
    while !revision_one.is_finished() {
        scripting.service(Some(&mut wm), &layout, output, false);
        std::thread::sleep(Duration::from_millis(1));
    }
    let (revision, catalog) = revision_one.join().unwrap();
    assert_eq!(revision, 1);
    assert_eq!(
        catalog
            .commands
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["focus-next", "restart-wm"]
    );
    for command in [
        ControlCommand {
            owner: ControlOwner::Policy,
            name: "focus-next".into(),
        },
        ControlCommand {
            owner: ControlOwner::Session,
            name: "restart-wm".into(),
        },
        ControlCommand {
            owner: ControlOwner::Session,
            name: "restart-wm".into(),
        },
    ] {
        let expected = if command.owner == ControlOwner::Policy {
            ControlOutcome::Committed
        } else {
            ControlOutcome::Completed
        };
        let committed_before = wm.committed;
        let epoch_before = wm.public.as_ref().unwrap().connection_epoch;
        let path = socket.clone();
        let client = std::thread::spawn(move || {
            ControlClient::connect(&path)
                .unwrap()
                .invoke(command)
                .unwrap()
                .1
        });
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut held_proposal = None;
        while held_proposal.is_none() {
            scripting.service(Some(&mut wm), &layout, output, false);
            wm.poll_restart(&mut layout, output).unwrap();
            held_proposal = wm.poll_request(&mut layout, output, true).unwrap();
            if client.is_finished() {
                panic!("premature result: {:?}", client.join().unwrap());
            }
            assert!(Instant::now() < deadline, "scripted proposal");
            std::thread::sleep(Duration::from_millis(1));
        }
        // A fully received policy proposal is still not a successful command.
        std::thread::sleep(Duration::from_millis(30));
        assert!(!client.is_finished());
        let result = layout.commit_proposal(held_proposal.unwrap());
        let settlement = wm.apply_commit_result(result, None, output.id).unwrap();
        assert!(
            settlement.physical_action.is_none(),
            "scripts cannot claim physical input evidence"
        );
        while !client.is_finished() {
            scripting.service(Some(&mut wm), &layout, output, false);
            assert!(Instant::now() < deadline, "terminal scripting outcome");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(client.join().unwrap(), expected);
        assert_eq!(wm.committed, committed_before + 1);
        if expected == ControlOutcome::Completed {
            assert_eq!(
                wm.public.as_ref().unwrap().connection_epoch,
                epoch_before + 1
            );
        }
    }
    // A queued action cannot follow a changed private mapping in the same
    // owner turn that accepted it, before catalog republication runs.
    let path = socket.clone();
    let stale = std::thread::spawn(move || {
        ControlClient::connect(&path)
            .unwrap()
            .invoke(ControlCommand {
                owner: ControlOwner::Policy,
                name: "focus-next".into(),
            })
            .unwrap()
            .1
    });
    let before = wm.committed;
    let deadline = Instant::now() + Duration::from_secs(3);
    while wm.public.as_ref().unwrap().control_tickets.is_empty() {
        scripting.service(Some(&mut wm), &layout, output, false);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    wm.public.as_mut().unwrap().control_generation = 0;
    while !stale.is_finished() {
        assert!(
            wm.poll_request(&mut layout, output, true)
                .unwrap()
                .is_none()
        );
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(stale.join().unwrap(), ControlOutcome::Stale);
    assert_eq!(wm.committed, before);

    // reload-profile raises the owner loop's reload request and settles only
    // on the reload owner's outcome, never on hand-off.
    scripting.service(Some(&mut wm), &layout, output, false);
    let path = socket.clone();
    let reload = std::thread::spawn(move || {
        ControlClient::connect(&path)
            .unwrap()
            .invoke(ControlCommand {
                owner: ControlOwner::Session,
                name: "reload-profile".into(),
            })
            .unwrap()
            .1
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut requested = SessionControlRequests::default();
    while !requested.reload_profile {
        scripting.service(Some(&mut wm), &layout, output, false);
        requested = scripting.take_session_requests();
        assert!(Instant::now() < deadline, "reload request raised");
        std::thread::sleep(Duration::from_millis(1));
    }
    std::thread::sleep(Duration::from_millis(30));
    assert!(!reload.is_finished(), "a handed-off reload is not settled");
    scripting.settle_reload(DesktopProfileReloadOutcome::Unchanged, &wm);
    while !reload.is_finished() {
        scripting.service(Some(&mut wm), &layout, output, false);
        assert!(Instant::now() < deadline, "reload settlement");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(reload.join().unwrap(), ControlOutcome::Unchanged);

    // logout raises the logout request and completes once admitted.
    let path = socket.clone();
    let logout = std::thread::spawn(move || {
        ControlClient::connect(&path)
            .unwrap()
            .invoke(ControlCommand {
                owner: ControlOwner::Session,
                name: "logout".into(),
            })
            .unwrap()
            .1
    });
    let mut requested = SessionControlRequests::default();
    while !logout.is_finished() {
        scripting.service(Some(&mut wm), &layout, output, false);
        let taken = scripting.take_session_requests();
        requested.logout |= taken.logout;
        assert!(Instant::now() < deadline, "logout settlement");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(requested.logout);
    assert_eq!(logout.join().unwrap(), ControlOutcome::Completed);
    drop(scripting);
    drop(wm);
    std::fs::remove_dir_all(root).unwrap();
}

fn pump(
    wm: &mut LiveWmSession,
    layout: &mut PersistentLiveLayout,
    output: sophia_engine::HeadlessOutput,
) {
    wm.poll_restart(layout, output).unwrap();
    if let Some(proposal) = wm.poll_request(layout, output, true).unwrap() {
        let result = layout.commit_proposal(proposal);
        wm.apply_commit_result(result, None, output.id).unwrap();
    }
    std::thread::sleep(Duration::from_millis(1));
}
