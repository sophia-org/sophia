#![cfg(target_os = "linux")]
//! t295: the pinned C SDK's lock provider client against the production lock
//! export. `lock_files_c.rs` proves the contract with hand-encoded records;
//! this test proves the vendored SDK client itself: negotiation with a chord,
//! lock object fetches, a sealed image, demand and permit, a presented
//! candidate, entry and chord events, and the republish to unlocking. The
//! test plays Session through the production worker.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use sophia_protocol::lock_files::*;
use sophia_runtime::lock_files::*;
use sophia_runtime::{
    ProcessLaunchSpec, ProcessSupervisor, SupervisedProcessKind, SupervisorCommand,
};
#[path = "support/image_test_authority.rs"]
mod authority;

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("lock-client-c-{label}-{}", std::process::id()));
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

fn compile(directory: &Path, source: Option<&Path>) -> PathBuf {
    let sdk = source.map(|p| p.join("src")).unwrap_or_else(|| {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/c-desktop-sdk/source/src")
    });
    let binary = directory.join("lock_client_c_sdk");
    let mut sources: Vec<_> = std::fs::read_dir(sdk.join("nine_p"))
        .unwrap()
        .chain(std::fs::read_dir(sdk.join("lock_files")).unwrap())
        .chain(std::fs::read_dir(sdk.join("image_channel")).unwrap())
        .chain(std::fs::read_dir(sdk.join("image_producer")).unwrap())
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
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/lock_client_c_sdk.c"))
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
fn the_pinned_c_sdk_lock_client_runs_against_the_production_export() {
    let scratch = Scratch::new("presented");
    let client = compile(&scratch.0, None);
    exchange(&scratch, &client, "c");
}

#[test]
#[ignore = "requires SOPHIA_LOCK_RUST_SDK_PEER, an independently built native Rust peer"]
fn rust_sdk_lock_client_runs_against_the_production_export() {
    let client = PathBuf::from(
        std::env::var_os("SOPHIA_LOCK_RUST_SDK_PEER").expect("native Rust peer path"),
    );
    assert!(client.is_absolute() && client.is_file());
    exchange(&Scratch::new("rust"), &client, "rust");
}

#[test]
#[ignore = "requires SOPHIA_LOCK_C_SDK_SOURCE, a candidate C SDK checkout"]
fn candidate_c_sdk_lock_client_runs_against_the_production_export() {
    let source =
        PathBuf::from(std::env::var_os("SOPHIA_LOCK_C_SDK_SOURCE").expect("candidate C source"));
    assert!(source.is_absolute() && source.join("src/sophia_lock_client.h").is_file());
    let scratch = Scratch::new("candidate-c");
    let client = compile(&scratch.0, Some(&source));
    exchange(&scratch, &client, "c");
}

fn exchange(scratch: &Scratch, client: &Path, language: &str) {
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
    transport
        .enable_isb_images(LockImagePolicy {
            hello_timeout_ms: 2000,
            validation_timeout_ms: 5000,
            validation_chunk_bytes: 65536,
            capture_interest_timeout_ms: 500,
            max_sealed_backing_bytes: 256,
        })
        .unwrap();
    let gate = scratch.0.join("ready");
    let spec = ProcessLaunchSpec::new(client)
        .arg(transport.socket_path())
        .env("SOPHIA_TEST_READY_FILE", &gate);
    let mut child = ProcessSupervisor::new(SupervisedProcessKind::LockProvider, spec);
    child
        .apply(SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::LockProvider,
            delay: Duration::ZERO,
        })
        .unwrap();
    // Supplied afid admission isolates SDK interoperability from launch auth.
    transport
        .authorize_assignee(
            LockFileAssignee::from_supervisor(&child)
                .unwrap()
                .with_authentication(Box::new(authority::Factory)),
        )
        .unwrap();
    let service = LockFileService::spawn(transport, locked(), Vec::new()).unwrap();
    // Admitted: release the client's gate.
    std::fs::write(gate, b"G").unwrap();

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut chords = None;
    let mut image = None;
    let mut presented = false;
    let status = loop {
        if Instant::now() >= deadline {
            let _ = child.terminate();
            panic!("the {language} SDK client did not finish");
        }
        if child.poll().unwrap().is_some() {
            break child.take_exit_status().unwrap();
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
                } => image = Some((width_px, height_px, pixels.to_vec())),
                LockInbound::Demand(demand) => service
                    .command(LockFileServiceCommand::Permit {
                        allocation_id: demand.allocation_id,
                        demand_id: demand.demand_id,
                        expires_after: Duration::from_millis(250),
                    })
                    .unwrap(),
                LockInbound::Candidate { candidate, .. } => {
                    presented = true;
                    service
                        .command(LockFileServiceCommand::Outcome(LockCandidateOutcome {
                            transaction: candidate.transaction,
                            lock_epoch: candidate.lock_epoch,
                            output_id: candidate.output_id,
                            allocation_id: candidate.allocation_id,
                            candidate_generation: candidate.candidate_generation,
                            status: LockCandidateStatus::Presented,
                            reason: 0,
                        }))
                        .unwrap();
                    service
                        .command(LockFileServiceCommand::Entry(LockEntry {
                            lock_epoch: 3,
                            entry: LockEntryKind::Insert,
                            empty_after: false,
                        }))
                        .unwrap();
                    service
                        .command(LockFileServiceCommand::Chord(LockChord {
                            lock_epoch: 3,
                            chord: 0,
                        }))
                        .unwrap();
                    service
                        .command(LockFileServiceCommand::PublishLock(LockObject {
                            lock_epoch: 3,
                            topology_generation: 1,
                            phase: LockPhase::Unlocking,
                            allocations: Vec::new(),
                        }))
                        .unwrap();
                }
                _ => {}
            },
            // The client leaves once it has seen the unlocking object.
            Ok(LockFileServiceEvent::Disconnected { .. }) => {}
            Ok(other) => panic!("unexpected service event: {other:?}"),
            Err(_) => {}
        }
    };
    assert!(status.success(), "{language} peer: {status:?}");
    assert_eq!(
        chords,
        Some(vec![LockChordRequest {
            keysym: 0x62,
            modifiers: 0b0100,
        }])
    );
    assert_eq!(
        image,
        Some((2, 2, vec![0x7f; 16])),
        "the image reached Session whole"
    );
    assert!(presented, "the candidate reached Session");
}
