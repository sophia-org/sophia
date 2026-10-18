//! Real protected launch, Factotum and lock service. Lock state and Session's
//! private-channel class are supplied; this does not run the live startup owner.
use super::lock_authenticated_transport::{Directory, Peer};
use super::*;
use sophia_protocol::lock_files::*;
use sophia_runtime::{ProtectionPath, lock_files::*};
use sophia_session::launch_authentication::files::TransportBudget;
use std::os::unix::fs::DirBuilderExt;

#[test]
#[ignore = "requires SOPHIA_LOCK_AUTH_PEER naming an absolute built SDK peer"]
fn external_sdk_lock_authenticates_and_observes_revocation() {
    exchange(false, false);
}

#[test]
#[ignore = "requires SOPHIA_LOCK_AUTH_PEER naming an absolute built SDK peer"]
fn external_sdk_lock_revocation_discards_unsubmitted_isb() {
    exchange(true, false);
}

#[test]
#[ignore = "requires SOPHIA_LOCK_AUTH_PEER naming an absolute built SDK peer"]
fn external_sdk_lock_replacement_gets_fresh_epoch_after_unsubmitted_isb() {
    exchange(true, true);
}

fn exchange(upload: bool, replacement: bool) {
    let executable = PathBuf::from(std::env::var_os("SOPHIA_LOCK_AUTH_PEER").unwrap());
    assert!(executable.is_absolute() && executable.is_file());
    let peer = Peer::external_sdk();
    let root = Directory(std::env::temp_dir().join(format!(
        "t333-lock-sdk-{}-{upload}-{replacement}",
        std::process::id()
    )));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root.0)
        .unwrap();
    let reports = root.0.join("reports");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&reports)
        .unwrap();
    let mut transport = LockFileTransport::bind_for_supervised_uid(
        root.0.join("endpoint"),
        rustix::process::geteuid().as_raw(),
        7,
        LockFileLimits {
            max_outputs: 1,
            upload_slots: 1,
            max_chords: 0,
            max_width_px: 8,
            max_height_px: 8,
            max_resource_bytes: 256,
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
            max_sealed_backing_bytes: 512,
        })
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let service = LockFileService::spawn(
        transport,
        LockObject {
            lock_epoch: 3,
            topology_generation: 1,
            phase: LockPhase::Locked,
            allocations: Vec::new(),
        },
        Vec::new(),
    )
    .unwrap();
    let mut f = Fixture::new();
    for round in 0..if replacement { 2 } else { 1 } {
        let epoch = 7 + round;
        let report = reports.join(format!("progress-{round}"));
        let upload = upload && round == 0;
        let id = f
            .book
            .reserve(
                LaunchPolicy {
                    principal: 8,
                    namespace: 9,
                    audience: 12,
                    role: ProtectionDomainRole::LockProvider,
                },
                Instant::now(),
                deadline(),
            )
            .unwrap();
        let end = deadline();
        while f.book.phase(id) != Some(Phase::Ready) {
            f.tick(end);
        }
        let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::LockProvider])
            .unwrap()
            .path(ProtectionPath::read_only(root.0.join("endpoint")))
            .unwrap()
            .path(ProtectionPath::read_write(&reports))
            .unwrap()
            .path(ProtectionPath::read_only(&executable))
            .unwrap();
        let mut supervisor = ProcessSupervisor::new(
            SupervisedProcessKind::LockProvider,
            ProcessLaunchSpec::new(&peer.0)
                .env("SOPHIA_TEST_LOCK_IMAGE", if upload { "1" } else { "0" })
                .env("SOPHIA_TEST_LOCK_EPOCH", epoch.to_string())
                .arg(&executable)
                .arg(&socket)
                .arg(&report)
                .protection_domain(domain),
        );
        supervisor
            .start_with_launch_channel(f.book.take_channel(id, Instant::now()).unwrap())
            .unwrap();
        f.activate(id, &mut supervisor);
        let worker = f
            .book
            .worker_authority(id, &mut supervisor, Instant::now())
            .unwrap();
        let factory = TransportBudget::new(f.namespaces.clone())
            .endpoint()
            .bind(worker, b"lock".to_vec(), b"lock-provider".to_vec())
            .unwrap();
        service
            .command(LockFileServiceCommand::ReplaceSupervisedProcess(
                LockFileAssignee::from_supervisor(&supervisor)
                    .unwrap()
                    .with_authentication(Box::new(factory)),
            ))
            .unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        let mut connected = 0;
        let mut disconnected = 0;
        let mut revoked = false;
        let mut exited = false;
        loop {
            f.tick(end);
            while let Some(event) = service.try_event().unwrap() {
                match event {
                    LockFileServiceEvent::Connected {
                        connection_epoch,
                        chords,
                    } => {
                        assert_eq!(connection_epoch, epoch);
                        assert!(chords.is_empty());
                        connected += 1;
                    }
                    LockFileServiceEvent::Disconnected { connection_epoch } => {
                        assert_eq!(connection_epoch, epoch);
                        disconnected += 1;
                    }
                    LockFileServiceEvent::Inbound { inbound, .. } => {
                        panic!(
                            "unsubmitted image must not deliver resource or candidate: {inbound:?}"
                        )
                    }
                    other => panic!("unexpected authenticated lock event: {other:?}"),
                }
            }
            if !revoked
                && connected == 1
                && std::fs::read_to_string(&report).ok().as_deref()
                    == Some(if upload {
                        "image-waiting\n"
                    } else {
                        "waiting\n"
                    })
            {
                assert_eq!(f.namespaces.lock().unwrap().admission_count(), 1);
                f.book.revoke(id, Instant::now()).unwrap();
                revoked = true;
                if upload {
                    std::fs::write(&report, b"continue\n").unwrap();
                }
            }
            exited |= supervisor.poll().unwrap().is_some();
            if exited && disconnected == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            supervisor.take_exit_status().unwrap().success(),
            "protected lock client must finish its authenticated exchange"
        );
        assert!(revoked);
        assert_eq!(connected, 1);
        assert_eq!(
            std::fs::read_to_string(&report).unwrap(),
            if upload {
                "image-revoked\n"
            } else {
                "revoked\n"
            }
        );
        assert_eq!(f.namespaces.lock().unwrap().admission_count(), 0);
    }
}
