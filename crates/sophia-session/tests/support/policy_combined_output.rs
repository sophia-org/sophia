//! Real Session restart entrypoints with file WM and existing output IPC in one
//! supervised process. Native mode only selects the fixture bootstrap: topology
//! is supplied, capabilities are empty, and no native device is constructed.
//! No pending topology candidate, rollback, frame or retirement is simulated.
use super::*;
use sophia_protocol::*;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::{Duration, Instant};

const CHILD: &str = "live_session::reload::tests::desktop_launch_reload::policy_combined_output::combined_role_child";

fn snapshot() -> OutputAuthoritySnapshot {
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

fn read_frame(stream: &mut UnixStream) -> Vec<u8> {
    let mut header = [0; SOPHIA_IPC_HEADER_LEN];
    stream.read_exact(&mut header).unwrap();
    let payload = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    assert!(payload <= 1024 * 1024, "bounded output fixture frame");
    let mut bytes = header.to_vec();
    bytes.resize(SOPHIA_IPC_HEADER_LEN + payload, 0);
    stream
        .read_exact(&mut bytes[SOPHIA_IPC_HEADER_LEN..])
        .unwrap();
    bytes
}

#[test]
#[ignore = "protected combined-role child invoked by its Session parent"]
fn combined_role_child() {
    assert!(std::env::var_os("SOPHIA_WM_SOCKET").is_none());
    let wm_path = std::env::var_os("SOPHIA_WM_9P_SOCKET").unwrap();
    let (limits, wm_epoch, qid, _wm) = policy_transport_worker::ninep::selection_peer::startup(
        UnixStream::connect(wm_path).unwrap(),
    );
    assert!(limits.profile_required);
    let mut output =
        UnixStream::connect(std::env::var_os("SOPHIA_OUTPUT_SOCKET").unwrap()).unwrap();
    output
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    output
        .set_write_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    output
        .write_all(
            &encode_output_v1_client_hello_frame(OutputV1ClientHello {
                minimum_revision: 1,
                maximum_revision: 1,
                capabilities: SOPHIA_OUTPUT_CAPABILITY_OBSERVE,
            })
            .unwrap(),
        )
        .unwrap();
    let welcome = decode_output_v1_server_welcome_frame(&read_frame(&mut output)).unwrap();
    let (_, received) = decode_output_v1_snapshot_frame(&read_frame(&mut output)).unwrap();
    assert_eq!(received.connection_epoch, welcome.connection_epoch);
    assert_eq!(received.snapshot, snapshot());
    let checkpoint = PathBuf::from(std::env::var_os("HAGIA_POLICY_CHECKPOINT").unwrap());
    // This third socket is only a fixture witness: parent SO_PEERCRED measures
    // the child in its own PID namespace rather than trusting the child's PID.
    let mut witness =
        UnixStream::connect(checkpoint.parent().unwrap().join("role-witness.sock")).unwrap();
    witness
        .set_write_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    witness
        .write_all(
            &[
                wm_epoch.to_le_bytes(),
                qid.to_le_bytes(),
                welcome.connection_epoch.to_le_bytes(),
            ]
            .concat(),
        )
        .unwrap();
    // Both admitted role streams remain held by this same process. The actual
    // Session restart terminates it; there is no fixture replacement owner.
    std::thread::sleep(Duration::from_secs(20));
}

fn observe_roles(wm: &mut LiveWmSession, listener: &UnixListener, epoch: u64) -> (u32, u64, u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut witness = loop {
        wm.poll_output_authority().unwrap();
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "both role negotiations must complete"
                );
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
    assert_eq!(wm.supervisor.protection_evidence().unwrap().peer_pid, pid);
    let mut record = [0; 24];
    witness.read_exact(&mut record).unwrap();
    let values: Vec<u64> = record
        .chunks_exact(8)
        .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
        .collect();
    assert_eq!(values[0], epoch);
    assert_eq!(wm.public.as_ref().unwrap().connection_epoch, epoch);
    loop {
        wm.poll_output_authority().unwrap();
        let public = wm.public.as_ref().unwrap();
        let authority = public.output_authority.as_ref().unwrap();
        if authority.connection_epoch() == values[2] {
            assert_eq!(authority.published(), &snapshot());
            assert!(public.output_service.is_some());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "production output owner must adopt replacement epoch"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    (pid, values[1], values[2])
}

#[test]
fn file_and_output_roles_share_the_supervised_replacement_across_both_entrypoints() {
    let mut source = ConfigFixture::new(&[]);
    source.config.wm_socket_path = source.directory.join("combined-wm.sock");
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
    let output = sophia_engine::HeadlessOutput::deterministic();
    let bootstrap = LiveOutputAuthorityBootstrap {
        snapshot: snapshot(),
        capabilities: vec![],
        startup_candidate: None,
    };
    let mut wm = LiveWmSession::from_started_public_config(
        &source.config,
        &[output],
        started,
        Some(bootstrap),
    )
    .unwrap();
    let first = observe_roles(&mut wm, &listener, 1);
    let mut layout = PersistentLiveLayout::default();
    wm.force_transport_restart = true;
    wm.poll_public_restart(&mut layout, output).unwrap();
    let second = observe_roles(&mut wm, &listener, 2);
    assert_ne!(second.0, first.0);
    assert!(second.1 > first.1 && second.2 > first.2);
    assert_eq!(wm.begin_control_restart(output).unwrap(), 3);
    let deadline = Instant::now() + Duration::from_secs(5);
    while wm.control_restart.is_some() {
        assert!(Instant::now() < deadline, "control restart deadline");
        wm.poll_control_restart(&mut layout, output);
        std::thread::sleep(Duration::from_millis(2));
    }
    let third = observe_roles(&mut wm, &listener, 3);
    assert_ne!(third.0, first.0);
    assert_ne!(third.0, second.0);
    assert!(third.1 > second.1 && third.2 > second.2);
    assert_eq!(wm.policy_wire_name(), "sophia_wm_fs_v1");
    assert!(!wm.public.as_ref().unwrap().configured);
}
