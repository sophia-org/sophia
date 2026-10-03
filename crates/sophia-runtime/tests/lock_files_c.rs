#![cfg(target_os = "linux")]
//! t294: an independent C peer exchanges records with the production lock
//! export. It uses only the C SDK's generic 9P client and encodes every lock
//! record from the KDL contract by hand; this test plays Session through the
//! production worker.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sophia_protocol::lock_files::*;
use sophia_runtime::lock_files::*;

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("lock-files-c-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn compile(directory: &Path) -> PathBuf {
    let sdk = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/c-desktop-sdk/source/src");
    let binary = directory.join("lock_files_peer");
    let mut sources: Vec<_> = std::fs::read_dir(sdk.join("nine_p"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "c"))
        .collect();
    sources.sort();
    let status = Command::new(std::env::var_os("CC").unwrap_or_else(|| "cc".into()))
        .args([
            "-std=c99",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-pedantic",
            "-UNDEBUG",
        ])
        .arg("-I")
        .arg(&sdk)
        .args(sources)
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/lock_files_peer.c"))
        .arg("-o")
        .arg(&binary)
        .status()
        .unwrap();
    assert!(status.success(), "C99 strict build failed");
    binary
}

fn locked() -> LockObject {
    LockObject {
        lock_epoch: 3,
        topology_generation: 1,
        phase: LockPhase::Locked,
        allocations: vec![LockAllocation {
            output_id: 1,
            output_generation: 1,
            allocation_id: 1,
            allocation_generation: 1,
            pixel_width: 2,
            pixel_height: 2,
            scale_numerator: 1,
            scale_denominator: 1,
        }],
    }
}

#[test]
fn an_independent_c_provider_negotiates_uploads_and_is_presented() {
    let scratch = Scratch::new("presented");
    let peer = compile(&scratch.0);
    let mut transport = LockFileTransport::bind_for_supervised_uid(
        scratch.0.join("endpoint"),
        rustix::process::geteuid().as_raw(),
        5,
        LockFileLimits {
            max_outputs: 1,
            upload_slots: 1,
            max_chords: 1,
            max_width_px: 4,
            max_height_px: 4,
            max_resource_bytes: 64,
            max_live_resources: 2,
            journal_records: 32,
            journal_bytes: 8192,
            assembly_timeout_ms: 2000,
            ack_progress_timeout_ms: 2000,
        },
    )
    .unwrap();
    let mut child = Command::new(&peer)
        .arg(transport.socket_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    transport.authorize_supervised_pid(child.id()).unwrap();
    let service = LockFileService::spawn(transport, locked(), Vec::new()).unwrap();
    // Admitted: release the peer's gate.
    child.stdin.take().unwrap().write_all(b"G").unwrap();

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut chords = None;
    let mut ready = false;
    let status = loop {
        assert!(Instant::now() < deadline, "the C peer did not finish");
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        match service.event_timeout(Duration::from_millis(5)) {
            Ok(LockFileServiceEvent::Connected {
                chords: granted, ..
            }) => chords = Some(granted),
            Ok(LockFileServiceEvent::Inbound { inbound, .. }) => match inbound {
                LockInbound::ResourceReady {
                    width_px,
                    height_px,
                    ref pixels,
                    ..
                } => {
                    assert_eq!((width_px, height_px), (2, 2));
                    assert_eq!(&**pixels, &[0x7f; 16]);
                    ready = true;
                }
                LockInbound::Demand(demand) => service
                    .command(LockFileServiceCommand::Permit {
                        allocation_id: demand.allocation_id,
                        demand_id: demand.demand_id,
                        expires_after: Duration::from_millis(250),
                    })
                    .unwrap(),
                LockInbound::Candidate { candidate, .. } => service
                    .command(LockFileServiceCommand::Outcome(LockCandidateOutcome {
                        transaction: candidate.transaction,
                        lock_epoch: candidate.lock_epoch,
                        output_id: candidate.output_id,
                        allocation_id: candidate.allocation_id,
                        candidate_generation: candidate.candidate_generation,
                        status: LockCandidateStatus::Presented,
                        reason: 0,
                    }))
                    .unwrap(),
                _ => {}
            },
            // The provider leaving once it has seen its outcome.
            Ok(LockFileServiceEvent::Disconnected { .. }) => {}
            Ok(other) => panic!("{other:?}"),
            Err(_) => {}
        }
    };
    let mut stdout = String::new();
    std::io::Read::read_to_string(child.stdout.as_mut().unwrap(), &mut stdout).unwrap();
    assert!(status.success(), "{status:?} {stdout}");
    assert!(
        stdout.contains("lock_files_peer status=presented"),
        "{stdout}"
    );
    assert_eq!(
        chords,
        Some(vec![LockChordRequest {
            keysym: 0x62,
            modifiers: 0b0100,
        }])
    );
    assert!(ready, "the image reached Session whole");
}
