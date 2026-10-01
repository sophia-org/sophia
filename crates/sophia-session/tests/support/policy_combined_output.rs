//! A profile-only WM through real Session launch and both restart entrypoints,
//! with the default WM transport and no output process. The WM receives no
//! output endpoint or output-authority grant. Session's native output
//! authority, including its startup transaction, has no listener and survives
//! every WM replacement unchanged. Native mode only selects the fixture
//! bootstrap: the capability is supplied and no native device is constructed;
//! dispatch is supplied; no effect is physically applied or presented.
use super::*;
use sophia_backend_live::{
    LibdrmNativeOutputCapability, LibdrmNativeOutputTiming, LibdrmNativeVrrPropertyDiscoveryStatus,
    project_live_output_authority_snapshot,
};
use sophia_protocol::*;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::{Duration, Instant};

const CHILD: &str = "live_session::reload::tests::desktop_launch_reload::policy_combined_output::profile_only_wm_child";
const STARTUP: u64 = u64::MAX;

/// A neutral supplied topology for fixtures that do not exercise native
/// capabilities.
pub(super) fn snapshot() -> OutputAuthoritySnapshot {
    OutputAuthoritySnapshot {
        topology_epoch: 1,
        primary_output: OutputId::from_raw(1),
        heads: vec![OutputHeadDescriptor {
            head: DisplayHeadId::from_raw(1),
            generation: 1,
            label: "Supplied head".into(),
            connected: true,
            enabled: true,
            current_mode: Some(DisplayModeId::from_raw(10)),
            transforms: OutputTransformSet::NORMAL,
            vrr_capable: false,
            modes: vec![OutputModeDescriptor {
                mode: DisplayModeId::from_raw(10),
                pixel_size: Size {
                    width: 1920,
                    height: 1080,
                },
                refresh_millihz: 60_000,
                preferred: true,
            }],
        }],
        groups: vec![OutputLogicalGroupState {
            output: OutputId::from_raw(1),
            generation: 1,
            logical: Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            members: vec![OutputGroupMember {
                head: DisplayHeadId::from_raw(1),
                mapping: OutputHeadMapping::Exact,
            }],
        }],
    }
}

#[test]
#[ignore = "protected profile-only WM child invoked by its Session parent"]
fn profile_only_wm_child() {
    for name in [
        "SOPHIA_WM_SOCKET",
        "SOPHIA_OUTPUT_SOCKET",
        "SOPHIA_OUTPUT_9P_SOCKET",
    ] {
        assert!(std::env::var_os(name).is_none(), "{name} reached the WM");
    }
    let wm_path = std::env::var_os("SOPHIA_WM_9P_SOCKET").unwrap();
    let (limits, wm_epoch, qid, _wm) = policy_transport_worker::ninep::selection_peer::startup(
        UnixStream::connect(wm_path).unwrap(),
    );
    assert!(limits.profile_required);
    let checkpoint = PathBuf::from(std::env::var_os("SOPHIA_WM_POLICY_CHECKPOINT").unwrap());
    // Fixture witness only: parent SO_PEERCRED measures the child in its own
    // PID namespace rather than trusting the child's PID.
    let mut witness =
        UnixStream::connect(checkpoint.parent().unwrap().join("role-witness.sock")).unwrap();
    witness
        .set_write_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    witness
        .write_all(&[wm_epoch.to_le_bytes(), qid.to_le_bytes()].concat())
        .unwrap();
    // The actual Session restart terminates this process.
    std::thread::sleep(Duration::from_secs(20));
}

/// One head with 60 and 75 Hz modes at the output's size, its projected
/// topology and a profile startup candidate selecting 75 Hz.
fn native_bootstrap(output: sophia_engine::HeadlessOutput) -> LiveOutputAuthorityBootstrap {
    let size = |refresh| {
        LibdrmNativeOutputTiming::new(
            u32::try_from(output.size.width).unwrap(),
            u32::try_from(output.size.height).unwrap(),
            refresh,
        )
    };
    let current = size(60_000);
    let capability = LibdrmNativeOutputCapability::new(
        output.id,
        11,
        "DP-1",
        [current, size(75_000)],
        Some(current),
        current,
        LibdrmNativeVrrPropertyDiscoveryStatus::Discovered,
    )
    .unwrap()
    .bind_head(sophia_engine::RenderHeadId::from_raw(11))
    .unwrap();
    let snapshot =
        project_live_output_authority_snapshot(std::slice::from_ref(&capability), &[output], 7)
            .unwrap();
    let head = &snapshot.heads[0];
    let group = &snapshot.groups[0];
    let alternate = head
        .modes
        .iter()
        .map(|mode| mode.mode)
        .find(|mode| Some(*mode) != head.current_mode)
        .unwrap();
    let startup_candidate = OutputTopologyCandidate {
        base_topology_epoch: snapshot.topology_epoch,
        intent: OutputTopologyIntent::Apply,
        primary_group_index: 0,
        heads: vec![OutputHeadTargetProposal {
            head: head.head,
            head_generation: head.generation,
            mode: alternate,
            transform: OutputTransform::Normal,
            vrr: OutputVrrPolicy::Disabled,
        }],
        groups: vec![OutputLogicalGroupProposal {
            output: group.output,
            logical: group.logical,
            members: group.members.clone(),
        }],
    };
    LiveOutputAuthorityBootstrap {
        snapshot,
        capabilities: vec![capability],
        startup_candidate: Some(startup_candidate),
    }
}

/// Session's output authority is exactly as bootstrapped: no service, the
/// first epoch, the original topology and the same startup effect custody.
fn assert_output_retained(wm: &LiveWmSession, expected: &OutputAuthoritySnapshot, dispatched: bool) {
    let public = wm.public.as_ref().unwrap();
    assert!(public.output_service.is_none(), "no output listener");
    let authority = public.output_authority.as_ref().unwrap();
    assert_eq!(authority.connection_epoch(), 1);
    assert_eq!(authority.published(), expected);
    assert_eq!(
        authority.active_transaction(),
        Some(TransactionId::from_raw(STARTUP))
    );
    assert_eq!(
        public.startup_output_transaction,
        Some(TransactionId::from_raw(STARTUP))
    );
    assert_eq!(public.output_topology_effect_pending(), !dispatched);
    assert_eq!(public.output_effect_dispatched, dispatched);
    assert!(public.output_pending_connection_epoch.is_none());
    assert!(public.output_cancel_requested.is_none());
}

fn observe_wm(
    wm: &mut LiveWmSession,
    listener: &UnixListener,
    epoch: u64,
    expected: &OutputAuthoritySnapshot,
    dispatched: bool,
) -> (u32, u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut witness = loop {
        wm.poll_output_authority().unwrap();
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "WM negotiation must complete");
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("role witness accept: {error}"),
        }
    };
    witness
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let credentials = rustix::net::sockopt::socket_peercred(&witness).unwrap();
    let pid = credentials.pid.as_raw_pid() as u32;
    assert_eq!(wm.supervisor.peer_id(), Some(pid));
    let evidence = wm.supervisor.protection_evidence().unwrap();
    assert_eq!(evidence.peer_pid, pid);
    assert!(
        !evidence
            .roles
            .contains(&sophia_runtime::ProtectionDomainRole::OutputAuthority),
        "the WM holds no output-authority grant"
    );
    let mut record = [0; 16];
    witness.read_exact(&mut record).unwrap();
    assert_eq!(u64::from_le_bytes(record[..8].try_into().unwrap()), epoch);
    assert_eq!(wm.public.as_ref().unwrap().connection_epoch, epoch);
    wm.poll_output_authority().unwrap();
    assert_output_retained(wm, expected, dispatched);
    (pid, u64::from_le_bytes(record[8..].try_into().unwrap()))
}

#[test]
fn profile_only_wm_restarts_keep_native_output_authority_without_a_listener() {
    profile_only_restarts(false);
}

#[test]
fn profile_only_wm_restarts_preserve_a_dispatched_output_effect() {
    profile_only_restarts(true);
}

fn profile_only_restarts(dispatched: bool) {
    let mut source = ConfigFixture::new(&[]);
    source.config.wm_socket_path = source.directory.join("profile-only-wm.sock");
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
    assert!(source.config.output_process.is_none());
    assert_eq!(source.config.wm_transport, WmTransportSelection::NineP2000L);
    // Fixture bootstrap selection only. Do not run native startup discovery.
    source.config.native_scanout = true;
    let prepared = LiveWmSession::prepare_public_launch(&mut source.config).unwrap();
    let key = sophia_config::DesktopProfileActivationKey::from(&source.config.desktop_profile);
    let checkpoint = prepared.as_ref().unwrap().directory.checkpoint_path();
    let parent = checkpoint.parent().unwrap();
    std::fs::write(
        parent.join("expected-profile"),
        [
            key.generation().raw().to_le_bytes().as_slice(),
            &key.digest().bytes(),
        ]
        .concat(),
    )
    .unwrap();
    let listener = UnixListener::bind(parent.join("role-witness.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let started = LiveWmSession::activate_public_launch(&mut source.config, prepared)
        .unwrap()
        .unwrap();
    assert!(
        started.runtime.output_transport.is_none(),
        "no output process, no output endpoint"
    );
    let output = sophia_engine::HeadlessOutput::deterministic();
    let bootstrap = native_bootstrap(output);
    let expected = bootstrap.snapshot.clone();
    let mut wm = LiveWmSession::from_started_public_config(
        &source.config,
        &[output],
        started,
        Some(bootstrap),
    )
    .unwrap();
    if dispatched {
        assert_eq!(wm.take_output_topology_effect().unwrap().transaction.raw(), STARTUP);
    }
    assert_output_retained(&wm, &expected, dispatched);
    let first = observe_wm(&mut wm, &listener, 1, &expected, dispatched);
    let mut layout = PersistentLiveLayout::default();
    wm.force_transport_restart = true;
    wm.poll_public_restart(&mut layout, output).unwrap();
    let second = observe_wm(&mut wm, &listener, 2, &expected, dispatched);
    assert_ne!(second.0, first.0);
    assert!(second.1 > first.1);
    assert_eq!(wm.begin_control_restart(output).unwrap(), 3);
    let deadline = Instant::now() + Duration::from_secs(5);
    while wm.control_restart.is_some() {
        assert!(Instant::now() < deadline, "control restart deadline");
        wm.poll_control_restart(&mut layout, output);
        std::thread::sleep(Duration::from_millis(2));
    }
    let third = observe_wm(&mut wm, &listener, 3, &expected, dispatched);
    assert_ne!(third.0, first.0);
    assert_ne!(third.0, second.0);
    assert!(third.1 > second.1);
    assert_eq!(wm.policy_wire_name(), "sophia_wm_fs_v1");
    assert!(!wm.public.as_ref().unwrap().configured);
}
