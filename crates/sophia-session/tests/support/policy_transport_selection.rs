//! Actual Session launch/restart entrypoints with a protected Rust child that
//! reads Limits and completes a scripted profile exchange against the staged
//! fragment. No Hagia policy or Engine settlement is claimed; output_service=None.
use super::*;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

const CHILD: &str = "live_session::reload::tests::desktop_launch_reload::policy_transport_selection::selected_transport_child";

#[test]
fn endpoint_factory_defaults_to_protected_files() {
    let source = ConfigFixture::new(&[]);
    assert_eq!(source.config.wm_transport, WmTransportSelection::NineP2000L);
    let key = sophia_config::DesktopProfileActivationKey::from(&source.config.desktop_profile);
    let directory = PreparedPublicPolicyLaunch::new(&source.config).unwrap();
    let transport =
        bind_public_policy_transport(&directory.directory, Some(key), source.config.wm_transport)
            .unwrap();
    assert!(matches!(transport, PublicPolicyTransport::Files(_)));
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
    let stream = UnixStream::connect(path).unwrap();
    let (limits, epoch, qid, _stream) = if std::env::var_os("SOPHIA_TEST_LOCK_SOCKET").is_some() {
        policy_transport_worker::ninep::selection_peer::authenticated_startup(stream)
    } else {
        policy_transport_worker::ninep::selection_peer::startup(stream)
    };
    assert!(limits.profile_required);
    assert_eq!(
        limits.capability_ceiling
            & (sophia_protocol::SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES
                | sophia_protocol::SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS),
        0
    );
    let checkpoint =
        Path::new(&std::env::var_os("SOPHIA_WM_POLICY_CHECKPOINT").unwrap()).to_owned();
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
    exercise_starts(false);
}

#[test]
fn normal_wm_startup_and_both_replacements_require_fresh_authenticated_launches() {
    exercise_starts(true);
}

#[test]
fn normal_wm_without_factotum_is_refused_before_launch() {
    let mut source = ConfigFixture::new(&[]);
    source.config.wm_process = Some(std::env::current_exe().unwrap().to_str().unwrap().into());
    let prepared = LiveWmSession::prepare_public_launch(&mut source.config).unwrap();
    assert!(prepared.is_some());
    source.config.factotum = None;
    let mut auth = crate::live_session::tests::lock_provider_factotum::Fixture::new();
    let wake = OwnerWake::new().unwrap();
    let error = LiveWmSession::activate_public_launch_authenticated(
        &mut source.config,
        prepared,
        WmStartupContext {
            launches: &mut auth.coordinator,
            namespace: sophia_protocol::NamespaceId::from_raw(2),
            wake: &wake,
            registration: None,
        },
    )
    .err()
    .expect("missing Factotum must not select unauthenticated WM startup");
    assert_eq!(
        error.to_string(),
        "public WM requires the Session authenticator"
    );
}

fn exercise_starts(authenticated: bool) {
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
    let wake = OwnerWake::new().unwrap();
    let mut auth =
        authenticated.then(crate::live_session::tests::lock_provider_factotum::Fixture::new);
    if authenticated {
        let executable = std::env::current_exe().unwrap();
        let bootstrap = PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap())
            .join("tmp/t317-normal-wm-bootstrap");
        std::fs::create_dir_all(bootstrap.parent().unwrap()).unwrap();
        assert!(
            std::process::Command::new("/usr/bin/cc")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-O2"])
                .arg(format!("-DSOPHIA_TEST_CHILD=\"{CHILD}\""))
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/support/lock_rust_bootstrap.c"
                ))
                .arg("-o")
                .arg(&bootstrap)
                .status()
                .unwrap()
                .success()
        );
        source.config.wm_process = Some(bootstrap.to_str().unwrap().into());
        source.config.wm_process_args = vec![
            executable.to_str().unwrap().into(),
            "/unused/socket".into(),
            "/unused/report".into(),
        ];
        source.config.wm_process_executable_grants.push(executable);
        source.config.factotum = Some(SessionFactotum {
            agent: "/unused/fixture-agent".into(),
            pam_helper: "/unused/fixture-helper".into(),
            pam_service: "fixture".into(),
            user: "fixture".into(),
        });
    }
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
    let started = if let Some(auth) = auth.as_mut() {
        LiveWmSession::activate_public_launch_authenticated(
            &mut source.config,
            prepared,
            WmStartupContext {
                launches: &mut auth.coordinator,
                namespace: sophia_protocol::NamespaceId::from_raw(2),
                wake: &wake,
                registration: None,
            },
        )
    } else {
        LiveWmSession::activate_public_launch(&mut source.config, prepared)
    }
    .unwrap()
    .unwrap();
    let output = sophia_engine::HeadlessOutput::deterministic();
    let mut wm =
        LiveWmSession::from_started_public_config(&source.config, &[output], started, None)
            .unwrap();
    assert!(wm.public.as_ref().unwrap().negotiated);
    assert!(
        !wm.pointer_focus_enabled(),
        "configuration is still pending"
    );
    let first = wait_marker(&wm, 1);
    let mut layout = PersistentLiveLayout::default();
    wm.force_transport_restart = true;
    if authenticated {
        // AbortSettlement can retire the worker before the child is reaped.
        wm.public.as_mut().unwrap().worker.take();
        assert!(
            wm.poll_public_request(&mut layout, output, true)
                .expect("retiring authenticated WM must not require a live worker")
                .is_none()
        );
        assert!(wm.public.as_ref().unwrap().in_flight_request.is_none());
    }
    wm.poll_public_restart(&mut layout, output).unwrap();
    if let Some(auth) = auth.as_mut() {
        drive_replacement(&mut wm, &mut layout, output, auth, &wake, 2);
    }
    assert_eq!(wm.public.as_ref().unwrap().connection_epoch, 2);
    let second = wait_marker(&wm, 2);
    assert!(second > first);
    assert_eq!(wm.begin_control_restart(output).unwrap(), 3);
    if let Some(auth) = auth.as_mut() {
        drive_replacement(&mut wm, &mut layout, output, auth, &wake, 3);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while wm.control_restart.is_some() && Instant::now() < deadline {
        wm.poll_control_restart(&mut layout, output);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(wm.control_restart.is_none(), "control restart deadline");
    assert_eq!(wm.public.as_ref().unwrap().connection_epoch, 3);
    let third = wait_marker(&wm, 3);
    assert!(third > second);
    if let Some(auth) = auth.as_mut() {
        // The profile-reload path replaces the supervisor specification. Keep
        // this peer's profile fixed so this tests launch custody, not policy.
        wm.pending_policy_launch_spec = Some(wm.supervisor.launch_spec().clone());
        wm.request_deliberate_restart();
        drive_replacement(&mut wm, &mut layout, output, auth, &wake, 4);
        assert!(wait_marker(&wm, 4) > third);
        let old = wm.authentication.as_ref().unwrap().registration.unwrap();
        auth.coordinator.revoke(old, Instant::now()).unwrap();
        wm.poll_wm_authentication(&mut auth.coordinator, wake.notifier(), &mut layout, output)
            .unwrap();
        assert!(
            wm.force_transport_restart,
            "retired launch authority must request WM replacement"
        );
        drive_replacement(&mut wm, &mut layout, output, auth, &wake, 5);
        assert!(
            auth.coordinator.ready(),
            "WM replacement must preserve the shared Factotum connection"
        );
    }
    assert_eq!(
        wm.public.as_ref().unwrap().wm_transport,
        WmTransportSelection::NineP2000L
    );
    assert_eq!(wm.policy_wire_name(), "sophia_wm_fs_v1");
    assert!(wm.public.as_ref().unwrap().output_service.is_none());
    assert!(!wm.public.as_ref().unwrap().configured);
}

fn drive_replacement(
    wm: &mut LiveWmSession,
    layout: &mut PersistentLiveLayout,
    output: sophia_engine::HeadlessOutput,
    auth: &mut crate::live_session::tests::lock_provider_factotum::Fixture,
    wake: &OwnerWake,
    epoch: u64,
) {
    let before = wm.authentication.as_ref().unwrap().registration;
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            Instant::now() < end,
            "authenticated WM replacement deadline"
        );
        auth.tick();
        wm.poll_public_restart(layout, output).unwrap();
        wm.poll_wm_authentication(&mut auth.coordinator, wake.notifier(), layout, output)
            .unwrap();
        assert!(!wm.degraded);
        if wm.public.as_ref().unwrap().connection_epoch == epoch {
            break;
        }
    }
    assert_ne!(
        wm.authentication.as_ref().unwrap().registration,
        before,
        "replacement must acquire a fresh launch registration"
    );
    // Authentication and profile negotiation continue after launch publication.
    let marker = wm
        .public
        .as_ref()
        .unwrap()
        .checkpoint_path
        .parent()
        .unwrap()
        .join(format!("transport-{epoch}"));
    while !marker.exists() {
        assert!(
            Instant::now() < end,
            "replacement authenticates and admits profile"
        );
        auth.tick();
        assert!(
            wm.poll_public_request(layout, output, false)
                .unwrap()
                .is_none()
        );
        assert!(
            !wm.force_transport_restart,
            "replacement WM must finish authenticated profile admission"
        );
    }
    while !wm.public.as_ref().unwrap().negotiated {
        assert!(
            Instant::now() < end,
            "owner observes authenticated profile admission"
        );
        auth.tick();
        assert!(
            wm.poll_public_request(layout, output, false)
                .unwrap()
                .is_none()
        );
    }
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
