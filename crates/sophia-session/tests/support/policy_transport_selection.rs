//! Actual Session launch/restart entrypoints with a protected Rust child that
//! reads Limits and completes a scripted profile exchange against the staged
//! fragment. No Hagia policy or Engine settlement is claimed; output_service=None.
use super::*;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

const CHILD: &str = "live_session::reload::tests::desktop_launch_reload::policy_transport_selection::selected_transport_child";

#[test]
fn endpoint_factory_preserves_default_ipc_and_requires_explicit_files() {
    let source = ConfigFixture::new(&[]);
    assert_eq!(source.config.wm_transport, WmTransportSelection::CurrentIpc);
    let key = sophia_config::DesktopProfileActivationKey::from(&source.config.desktop_profile);
    let directory = PreparedPublicPolicyLaunch::new(&source.config).unwrap();
    let transport =
        bind_public_policy_transport(&directory.directory, Some(key), source.config.wm_transport)
            .unwrap();
    assert!(matches!(transport, PublicPolicyTransport::CurrentIpc(_)));
    drop(transport);
    let transport = bind_public_policy_transport(
        &directory.directory,
        Some(key),
        WmTransportSelection::NineP2000L,
    )
    .unwrap();
    assert!(matches!(transport, PublicPolicyTransport::Files(_)));
}

#[test]
#[ignore = "protected fixture child, invoked by its Session parent"]
fn selected_transport_child() {
    let path = std::env::var_os("SOPHIA_WM_9P_SOCKET").expect("explicit file socket");
    assert!(std::env::var_os("SOPHIA_WM_SOCKET").is_none());
    let (limits, epoch, qid, _stream) = policy_transport_worker::ninep::selection_peer::startup(
        UnixStream::connect(path).unwrap(),
    );
    assert!(limits.profile_required);
    assert_eq!(
        limits.capability_ceiling
            & (sophia_protocol::SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES
                | sophia_protocol::SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS),
        0
    );
    let checkpoint = Path::new(&std::env::var_os("HAGIA_POLICY_CHECKPOINT").unwrap()).to_owned();
    let marker = checkpoint
        .parent()
        .unwrap()
        .join(format!("transport-{epoch}"));
    let temporary = marker.with_extension("pending");
    std::fs::write(&temporary, format!("{epoch} {qid}")).unwrap();
    std::fs::rename(temporary, marker).unwrap();
    // Keep connection and supervision alive until the parent chooses restart.
    std::thread::sleep(Duration::from_secs(20));
}

fn wait_marker(wm: &LiveWmSession, epoch: u64) -> u64 {
    let path = wm
        .public
        .as_ref()
        .unwrap()
        .checkpoint_path
        .parent()
        .unwrap()
        .join(format!("transport-{epoch}"));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let text =
        std::fs::read_to_string(path).expect("protected child must read selected endpoint Limits");
    let values: Vec<u64> = text
        .split_whitespace()
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(values[0], epoch);
    values[1]
}

#[test]
fn initial_automatic_and_control_starts_keep_file_selection_and_logical_qids() {
    let mut source = ConfigFixture::new(&[]);
    source.config.wm_socket_path = source.directory.join("selected-wm.sock");
    source.config.wm_process = Some(
        std::env::current_exe()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned(),
    );
    source.config.wm_process_args = vec![
        "--exact".into(),
        CHILD.into(),
        "--ignored".into(),
        "--nocapture".into(),
    ];
    source.config.wm_transport = WmTransportSelection::NineP2000L;
    source.config.native_scanout = false;
    let prepared = LiveWmSession::prepare_public_launch(&mut source.config).unwrap();
    let key = sophia_config::DesktopProfileActivationKey::from(&source.config.desktop_profile);
    let expected = [
        key.generation().raw().to_le_bytes().as_slice(),
        &key.digest().bytes(),
    ]
    .concat();
    std::fs::write(
        prepared
            .as_ref()
            .unwrap()
            .directory
            .checkpoint_path()
            .parent()
            .unwrap()
            .join("expected-profile"),
        expected,
    )
    .unwrap();
    let started = LiveWmSession::activate_public_launch(&mut source.config, prepared)
        .unwrap()
        .unwrap();
    let output = sophia_engine::HeadlessOutput::deterministic();
    let mut wm =
        LiveWmSession::from_started_public_config(&source.config, &[output], started, None)
            .unwrap();
    let first = wait_marker(&wm, 1);
    let mut layout = PersistentLiveLayout::default();
    wm.force_transport_restart = true;
    wm.poll_public_restart(&mut layout, output).unwrap();
    assert_eq!(wm.public.as_ref().unwrap().connection_epoch, 2);
    let second = wait_marker(&wm, 2);
    assert!(second > first);
    assert_eq!(wm.begin_control_restart(output).unwrap(), 3);
    let deadline = Instant::now() + Duration::from_secs(5);
    while wm.control_restart.is_some() && Instant::now() < deadline {
        wm.poll_control_restart(&mut layout, output);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(wm.control_restart.is_none(), "control restart deadline");
    assert_eq!(wm.public.as_ref().unwrap().connection_epoch, 3);
    let third = wait_marker(&wm, 3);
    assert!(third > second);
    assert_eq!(
        wm.public.as_ref().unwrap().wm_transport,
        WmTransportSelection::NineP2000L
    );
    assert_eq!(wm.policy_wire_name(), "sophia_wm_fs_v1");
    assert!(wm.public.as_ref().unwrap().output_service.is_none());
    assert!(!wm.public.as_ref().unwrap().configured);
}

#[test]
fn file_profile_rollback_preserves_selection_and_exact_launch_spec() {
    let mut source = ConfigFixture::new(&[]);
    source.config.wm_transport = WmTransportSelection::NineP2000L;
    let mut fixture = ReloadFixture::from_config_fixture(source);
    fixture.save("/old/command", None);
    assert_eq!(fixture.reload(), DesktopProfileReloadOutcome::Applied);
    let old_spec = fixture.wm.supervisor.launch_spec().clone();
    fixture.save("/rejected/command", Some("grid"));
    assert_eq!(
        fixture.reload(),
        DesktopProfileReloadOutcome::RestartRequired
    );
    let restored = fixture.wm.rollback_desktop_reload().unwrap();
    assert_eq!(restored, old_spec);
    assert_eq!(
        fixture.wm.public.as_ref().unwrap().wm_transport,
        WmTransportSelection::NineP2000L
    );
    assert!(
        restored
            .environment
            .iter()
            .any(|(key, _)| key == "SOPHIA_WM_9P_SOCKET")
    );
    assert!(
        !restored
            .environment
            .iter()
            .any(|(key, _)| key == "SOPHIA_WM_SOCKET")
    );
}
