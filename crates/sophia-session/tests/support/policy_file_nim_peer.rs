//! Opt-in prebuilt independent peer. The accepted test socket is supplied
//! admission, and parent outcomes are scripted, never Engine/native receipts.
#![cfg(test)]
use super::super::startup::tests::{admission, profile};
use super::*;
use crate::live_session::policy_transport_worker::{PolicyTransportEvent, PolicyTransportWorker};
use sha2::{Digest, Sha256};
use sophia_protocol::*;
use std::fs::{self, File};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::sync_channel;

use super::super::startup::tests::array_fixture as fixture;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct PeerProcess {
    child: Child,
    socket_dir: PathBuf,
}
struct StartupThread {
    stop: Box<dyn PolicyAdapterStop>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for StartupThread {
    fn drop(&mut self) {
        self.stop.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Drop for PeerProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let deadline = Instant::now() + Duration::from_secs(2);
            while self.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            if self.child.try_wait().ok().flatten().is_none() {
                eprintln!(
                    "independent WM peer could not be reaped within cleanup budget: pid={}",
                    self.child.id()
                );
            }
        }
        let _ = fs::remove_dir_all(&self.socket_dir);
    }
}
fn hash(path: &Path) -> String {
    assert!(
        fs::metadata(path).unwrap().len() <= 64 * 1024 * 1024,
        "peer binary bound"
    );
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}
fn spawn_peer(case: &str) -> (PeerProcess, UnixStream, PathBuf, PathBuf, String) {
    let binary = fs::canonicalize(
        std::env::var_os("SOPHIA_WM_FILE_PEER").expect("required prebuilt Nim peer missing"),
    )
    .unwrap();
    let expected = std::env::var("SOPHIA_WM_FILE_PEER_SHA256")
        .expect("required independent peer hash missing");
    assert_eq!(expected.len(), 64);
    assert!(expected.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(
        hash(&binary),
        expected.to_ascii_lowercase(),
        "prebuilt peer identity"
    );
    let evidence = PathBuf::from(
        std::env::var_os("SOPHIA_WM_FILE_PEER_EVIDENCE")
            .expect("required new evidence directory missing"),
    )
    .join(case);
    fs::create_dir(&evidence).expect("case evidence must be fresh; parent must exist");
    let dir = std::env::temp_dir().join(format!(
        "swmf-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    let socket = dir.join("peer");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let child = Command::new(&binary)
        .arg(&socket)
        .arg(case)
        .stdin(Stdio::null())
        .stdout(File::create(evidence.join("stdout.log")).unwrap())
        .stderr(File::create(evidence.join("stderr.log")).unwrap())
        .spawn()
        .unwrap();
    let mut child = PeerProcess {
        child,
        socket_dir: dir,
    };
    fs::write(evidence.join("identity.txt"),format!("binary={}\nsha256={}\npid={}\ncase={}\nidentity_method=path-hash-before-and-after\ndescriptor_pinned_exec=false\nhash_then_exec_window=true\nsupplied_admission=true\nscripted_outcomes=true\nnative_presentation=false\n",binary.display(),expected,child.child.id(),case)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("peer accept: {error}"),
        }
        assert!(
            child.child.try_wait().unwrap().is_none(),
            "peer exited before connect"
        );
        assert!(Instant::now() < deadline, "peer connect deadline");
        std::thread::sleep(Duration::from_millis(5));
    };
    let credentials = rustix::net::sockopt::socket_peercred(&stream).unwrap();
    let measured_pid = credentials.pid.as_raw_pid() as u32;
    assert_eq!(
        measured_pid,
        child.child.id(),
        "accepted peer must be spawned child"
    );
    use std::io::Write;
    writeln!(fs::OpenOptions::new().append(true).open(evidence.join("identity.txt")).unwrap(),
        "measured_peer_pid={measured_pid}\nmeasured_peer_uid={}\npeer_pid_matches_spawned_child=true", credentials.uid.as_raw()).unwrap();
    (
        child,
        stream,
        evidence,
        binary,
        expected.to_ascii_lowercase(),
    )
}
fn finish(peer: &mut PeerProcess, evidence: &Path, binary: &Path, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = peer.child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "peer exit deadline");
        std::thread::sleep(Duration::from_millis(5));
    };
    fs::write(evidence.join("exit.txt"), format!("{status}\n")).unwrap();
    assert!(
        status.success(),
        "independent peer refused; inspect retained logs"
    );
    assert_eq!(hash(binary), expected, "peer artifact changed during run");
}

#[test]
#[ignore = "requires pinned prebuilt Nim WM file peer and fresh evidence directory"]
fn independent_nim_supplied_stream_startup() {
    let (mut peer, stream, evidence, binary, expected) = spawn_peer("startup");
    let caps = sophia_runtime::select_policy_capabilities(u64::MAX, u64::MAX, true);
    let mut startup = FileStartup::adopt(
        stream,
        9,
        WmFileLimits {
            capability_ceiling: caps,
            profile_required: true,
        },
        WmQids::new(),
    )
    .unwrap();
    let stop = startup.stop_handle();
    let (done, result) = sync_channel(1);
    let thread = std::thread::spawn(move || {
        let value = startup.admit(admission(), Some(profile()));
        done.send(value).unwrap();
        while let Ok(reactor) = startup.reactor_mut() {
            if reactor.turn(Duration::from_millis(100)).is_err() {
                break;
            }
        }
        startup.close();
    });
    let running = StartupThread {
        stop,
        thread: Some(thread),
    };
    let admitted = result.recv_timeout(Duration::from_secs(20));
    if !matches!(&admitted, Ok(Ok(()))) {
        running.stop.stop();
    }
    // Stop/child cleanup is owned by this fixture even on a failed assertion.
    if admitted.as_ref().is_ok_and(|r| r.is_ok()) {
        finish(&mut peer, &evidence, &binary, &expected);
    }
    drop(running);
    admitted.unwrap().unwrap();
}

#[test]
#[ignore = "requires pinned prebuilt Nim WM file peer and fresh evidence directory"]
fn independent_nim_supplied_stream_cycle() {
    let (mut peer, stream, evidence, binary, expected) = spawn_peer("cycle");
    let caps = sophia_runtime::select_policy_capabilities(u64::MAX, u64::MAX, true);
    let adapter = NinePPolicyAdapter::supplied(
        stream,
        9,
        WmFileLimits {
            capability_ceiling: caps,
            profile_required: true,
        },
        WmQids::new(),
    )
    .unwrap();
    let worker = PolicyTransportWorker::spawn(adapter, 9, Some(profile())).unwrap();
    assert!(matches!(
        worker.event_timeout(Duration::from_secs(20)).unwrap(),
        PolicyTransportEvent::Negotiated
    ));
    let PolicyTransportEvent::Configuration {
        transaction,
        configuration,
    } = worker.event_timeout(Duration::from_secs(3)).unwrap()
    else {
        panic!("configuration missing")
    };
    assert_eq!(transaction, TransactionId::from_raw(10));
    assert_eq!(configuration.generation, 3);
    assert!(
        worker
            .try_command(PolicyTransportCommand::ConfigurationOutcome {
                transaction,
                generation: configuration.generation,
                outcome: PolicyProjectionOutcome::Committed
            })
            .is_ok()
    );
    assert!(matches!(
        worker.event_timeout(Duration::from_secs(3)).unwrap(),
        PolicyTransportEvent::ReadyForCycle { .. }
    ));
    let scene = fixture::scene();
    let generation = scene.generation;
    let request = PolicyProjectionRequest {
        connection_epoch: 9,
        request_id: 55,
        scene_generation: generation,
        policy_generation: configuration.generation,
        affected_outputs: vec![scene.active_output],
        cause: PolicyRequestCause::SceneChanged,
    };
    assert!(
        worker
            .try_command(PolicyTransportCommand::Cycle {
                snapshot_transaction: TransactionId::from_raw(100),
                request_transaction: TransactionId::from_raw(101),
                scene: Box::new(scene),
                actions: configuration.actions,
                classifications: vec![],
                launch_origins: vec![],
                request
            })
            .is_ok()
    );
    let PolicyTransportEvent::Projection(proposal) =
        worker.event_timeout(Duration::from_secs(5)).unwrap()
    else {
        panic!("projection missing")
    };
    assert_eq!(proposal.connection_epoch, 9);
    assert_eq!(proposal.transaction, TransactionId::from_raw(11));
    assert_eq!(proposal.request_id, 55);
    assert_eq!(proposal.base_generation, generation);
    assert_eq!(proposal.active_output, OutputId::from_raw(1));
    assert_eq!(proposal.outputs.len(), 1);
    let output = &proposal.outputs[0];
    assert_eq!(output.output, OutputId::from_raw(1));
    assert_eq!(output.focus, Some(SurfaceId::new(3, 1)));
    assert_eq!(output.placements.len(), 1);
    let placement = &output.placements[0];
    assert_eq!(placement.surface, SurfaceId::new(3, 1));
    assert_eq!(placement.surface_generation, 8);
    assert_eq!(placement.geometry, fixture::rect());
    assert!(
        worker
            .try_command(PolicyTransportCommand::ProjectionOutcome {
                transaction: proposal.transaction,
                request_id: 55,
                scene_generation: generation,
                outcome: PolicyProjectionOutcome::Committed,
                expect_session_operation: true
            })
            .is_ok()
    );
    let PolicyTransportEvent::SessionOperation {
        transaction,
        request,
    } = worker.event_timeout(Duration::from_secs(5)).unwrap()
    else {
        panic!("session operation missing")
    };
    assert_eq!(transaction, TransactionId::from_raw(12));
    assert_eq!(request.connection_epoch, 9);
    assert_eq!(request.request_id, 55);
    assert_eq!(request.operation, 1);
    assert_eq!(request.target, None);
    assert!(
        worker
            .try_command(PolicyTransportCommand::SessionOperationOutcome {
                transaction,
                request_id: 55,
                outcome: PolicyProjectionOutcome::Committed,
            })
            .is_ok()
    );
    assert!(matches!(
        worker.event_timeout(Duration::from_secs(3)).unwrap(),
        PolicyTransportEvent::ReadyForCycle { .. }
    ));
    assert!(
        worker
            .try_command(PolicyTransportCommand::PresentationReceipt {
                transaction: TransactionId::from_raw(102),
                receipt: PolicyPresentationReceipt {
                    connection_epoch: 9,
                    publication_generation: 1,
                    output: OutputId::from_raw(1),
                    output_generation: 3,
                    presentation_epoch: 1,
                    outcome: PolicyPresentationOutcome::Presented
                }
            })
            .is_ok()
    );
    finish(&mut peer, &evidence, &binary, &expected);
    drop(worker);
}
