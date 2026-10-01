//! Separate protected output process through real Session launch/restart.
//! Supplied topology only: no native device, apply or presentation evidence.
use super::*;
use sophia_protocol::output_files::*;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::{Duration, Instant};

use crate::live_session::tests::shell_file_peer as raw_peer;

const WM: &str = "live_session::reload::tests::desktop_launch_reload::output_file_launch::wm_child";
const OUTPUT: &str =
    "live_session::reload::tests::desktop_launch_reload::output_file_launch::output_child";

#[test]
#[ignore = "protected WM fixture, invoked by its Session parent"]
fn wm_child() {
    assert!(std::env::var_os("SOPHIA_OUTPUT_SOCKET").is_none());
    assert!(std::env::var_os("SOPHIA_OUTPUT_9P_SOCKET").is_none());
    let path = std::env::var_os("SOPHIA_WM_9P_SOCKET").unwrap();
    let (_, epoch, _, _stream) =
        policy_transport_worker::ninep::selection_peer::startup(UnixStream::connect(path).unwrap());
    let checkpoint = PathBuf::from(std::env::var_os("SOPHIA_WM_POLICY_CHECKPOINT").unwrap());
    std::fs::write(
        checkpoint
            .parent()
            .unwrap()
            .join(format!("separate-wm-{epoch}")),
        b"ready",
    )
    .unwrap();
    std::thread::sleep(Duration::from_secs(20));
}

#[test]
#[ignore = "protected output fixture, invoked by its Session parent"]
fn output_child() {
    assert!(std::env::var_os("SOPHIA_WM_9P_SOCKET").is_none());
    assert!(std::env::var_os("SOPHIA_OUTPUT_SOCKET").is_none());
    let path = PathBuf::from(std::env::var_os("SOPHIA_OUTPUT_9P_SOCKET").unwrap());
    let mut peer = raw_peer::Peer::connect(&path);
    peer.setup();
    peer.open(6, b"limits", 0);
    let limits = peer.read(6, 0);
    let epoch = decode_output_file_record(&limits, OutputFileClass::Object)
        .unwrap()
        .header
        .connection_epoch;
    let candidate = encode_output_file_record(
        OutputFileHeader {
            kind: OutputFileKind::Negotiate,
            connection_epoch: epoch,
            submission_id: 1,
            sequence: 0,
        },
        &encode_output_file_negotiate(sophia_protocol::OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities: 1,
        }),
    )
    .unwrap();
    peer.open(5, b"transaction", 2);
    assert_eq!(peer.write(5, &candidate).0, 119);
    assert_eq!(
        peer.write(
            3,
            &encode_output_file_submit(OutputFileSubmit {
                connection_epoch: epoch,
                submission_id: 1,
                candidate_bytes: candidate.len() as u32,
            })
            .unwrap()
        )
        .0,
        119
    );
    let events = peer.read(2, 0);
    let publication = decode_output_file_record(&events[104..], OutputFileClass::Event).unwrap();
    let publication = decode_output_file_publication(publication.body).unwrap();
    peer.open(7, b"topology", 0);
    let original = peer.read(7, 0);
    assert_eq!(
        peer.write(
            4,
            &encode_output_file_ack(OutputFileAck {
                connection_epoch: epoch,
                sequence: 3,
            })
            .unwrap()
        )
        .0,
        119
    );
    peer.clunk(5);
    let mut witness = UnixStream::connect(path.parent().unwrap().join("witness.sock")).unwrap();
    witness
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    loop {
        witness.write_all(&epoch.to_le_bytes()).unwrap();
        witness
            .write_all(&publication.qid_path.to_le_bytes())
            .unwrap();
        let mut command = [0];
        witness.read_exact(&mut command).unwrap();
        if command == [b'X'] {
            break;
        }
        assert_eq!(peer.read(7, 0), original, "WM restart changed output pin");
    }
}

fn observe(
    wm: &mut LiveWmSession,
    witness: &mut UnixStream,
    wm_epoch: u64,
    output_pid: u32,
) -> (u64, u64) {
    let mut record = [0; 16];
    witness.read_exact(&mut record).unwrap();
    let epoch = u64::from_le_bytes(record[..8].try_into().unwrap());
    let qid = u64::from_le_bytes(record[8..].try_into().unwrap());
    assert_eq!(epoch, 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        wm.poll_output_authority().unwrap();
        let public = wm.public.as_ref().unwrap();
        if public
            .checkpoint_path
            .parent()
            .unwrap()
            .join(format!("separate-wm-{wm_epoch}"))
            .exists()
        {
            assert_eq!(
                public.output_authority.as_ref().unwrap().connection_epoch(),
                1
            );
            let LiveOutputService::Files { supervisor, .. } =
                public.output_service.as_ref().unwrap();
            assert_eq!(supervisor.peer_id(), Some(output_pid));
            assert_ne!(wm.supervisor.peer_id(), Some(output_pid));
            return (epoch, qid);
        }
        assert!(Instant::now() < deadline, "WM startup deadline");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn independent_output_process_survives_both_wm_restart_paths_without_reassignment() {
    let mut source = ConfigFixture::new(&[]);
    source.config.wm_socket_path = source.directory.join("separate-wm.sock");
    let executable = std::env::current_exe()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    source.config.wm_process = Some(executable.clone());
    source.config.output_process = Some(executable);
    let arguments = |child: &str| {
        vec![
            "--exact".into(),
            child.into(),
            "--ignored".into(),
            "--nocapture".into(),
        ]
    };
    source.config.wm_process_args = arguments(WM);
    source.config.output_process_args = arguments(OUTPUT);
    source.config.native_scanout = true; // Select supplied fixture bootstrap only.
    let prepared = LiveWmSession::prepare_public_launch(&mut source.config).unwrap();
    let key = sophia_config::DesktopProfileActivationKey::from(&source.config.desktop_profile);
    let checkpoint = prepared.as_ref().unwrap().directory.checkpoint_path();
    std::fs::write(
        checkpoint.parent().unwrap().join("expected-profile"),
        [
            key.generation().raw().to_le_bytes().as_slice(),
            &key.digest().bytes(),
        ]
        .concat(),
    )
    .unwrap();
    let started = LiveWmSession::activate_public_launch(&mut source.config, prepared)
        .unwrap()
        .unwrap();
    let PreparedOutputTransport::Files {
        transport,
        supervisor,
    } = started.runtime.output_transport.as_ref().unwrap();
    let pid = supervisor.peer_id().unwrap();
    assert_eq!(
        supervisor.protection_evidence().unwrap().roles,
        [sophia_runtime::ProtectionDomainRole::OutputAuthority]
            .into_iter()
            .collect()
    );
    assert!(
        !started
            .runtime
            .supervisor
            .protection_evidence()
            .unwrap()
            .roles
            .contains(&sophia_runtime::ProtectionDomainRole::OutputAuthority)
    );
    let listener = UnixListener::bind(
        transport
            .socket_path()
            .parent()
            .unwrap()
            .join("witness.sock"),
    )
    .unwrap();
    listener.set_nonblocking(true).unwrap();
    let output = sophia_engine::HeadlessOutput::deterministic();
    let mut wm = LiveWmSession::from_started_public_config(
        &source.config,
        &[output],
        started,
        Some(LiveOutputAuthorityBootstrap {
            snapshot: super::policy_combined_output::snapshot(),
            capabilities: vec![],
            startup_candidate: None,
        }),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut witness = loop {
        wm.poll_output_authority().unwrap();
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("witness: {error}"),
        }
    };
    witness
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let initial = observe(&mut wm, &mut witness, 1, pid);
    let mut layout = PersistentLiveLayout::default();
    wm.force_transport_restart = true;
    wm.poll_public_restart(&mut layout, output).unwrap();
    witness.write_all(b"R").unwrap();
    assert_eq!(observe(&mut wm, &mut witness, 2, pid), initial);
    wm.begin_control_restart(output).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while wm.control_restart.is_some() {
        assert!(Instant::now() < deadline);
        wm.poll_control_restart(&mut layout, output);
        std::thread::sleep(Duration::from_millis(2));
    }
    witness.write_all(b"R").unwrap();
    assert_eq!(observe(&mut wm, &mut witness, 3, pid), initial);
    witness.write_all(b"X").unwrap();
}
