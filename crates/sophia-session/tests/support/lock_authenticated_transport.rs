//! Shipped lock worker, real Factotum and a protected independent C afid peer.
use super::*;
use sophia_protocol::lock_files::*;
use sophia_runtime::{ProtectionPath, lock_files::*};
use sophia_session::launch_authentication::files::TransportBudget;
use std::os::unix::fs::DirBuilderExt;

struct Peer(PathBuf);
impl Peer {
    fn build(sdk: bool) -> Self {
        let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "lock-authentication-peer-{sdk}-{}",
            std::process::id()
        ));
        let mut command = std::process::Command::new("/usr/bin/cc");
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-O2"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/lock_authentication_peer.c"
            ))
            .arg("-o")
            .arg(&path);
        if sdk {
            let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../vendor/c-desktop-sdk/source/src");
            let mut units: Vec<_> = std::fs::read_dir(source.join("nine_p"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "c"))
                .collect();
            units.sort();
            command
                .arg("-DSOPHIA_TEST_C_SDK")
                .arg("-I")
                .arg(source)
                .args(units);
        }
        assert!(command.status().unwrap().success());
        Self(path)
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn protected_lock_worker_authenticates_reauthenticates_and_revokes_the_actual_transport() {
    run(false);
}

#[test]
fn c_sdk_authenticates_and_reauthenticates_the_protected_lock_transport() {
    run(true);
}

fn run(sdk: bool) {
    let peer = Peer::build(sdk);
    let root = Directory(
        std::env::temp_dir().join(format!("t317-lock-wire-{sdk}-{}", std::process::id())),
    );
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root.0)
        .unwrap();
    let reports = root.0.join("reports");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&reports)
        .unwrap();
    let report = reports.join("progress");
    let limits = LockFileLimits {
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
    };
    let transport = LockFileTransport::bind_for_supervised_uid(
        root.0.join("endpoint"),
        rustix::process::geteuid().as_raw(),
        7,
        limits,
    )
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
    let id = f
        .book
        .reserve(
            LaunchPolicy {
                principal: 8,
                namespace: 9,
                audience: 10,
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
        .path(ProtectionPath::read_only(socket.parent().unwrap()))
        .unwrap()
        .path(ProtectionPath::read_write(&reports))
        .unwrap();
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::LockProvider,
        ProcessLaunchSpec::new(&peer.0)
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
    let budget = TransportBudget::default();
    let factory = budget
        .endpoint()
        .bind(worker, b"lock".to_vec(), b"lock-provider".to_vec())
        .unwrap();
    let assignee = LockFileAssignee::from_supervisor(&supervisor)
        .unwrap()
        .with_authentication(Box::new(factory));
    service
        .command(LockFileServiceCommand::ReplaceSupervisedProcess(assignee))
        .unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let mut connected = 0;
    let mut disconnected = 0;
    let mut revoked = false;
    loop {
        f.tick(end);
        while let Some(event) = service.try_event().unwrap() {
            match event {
                LockFileServiceEvent::Connected {
                    connection_epoch,
                    chords,
                } => {
                    assert_eq!(connection_epoch, 7);
                    assert!(chords.is_empty());
                    connected += 1;
                }
                LockFileServiceEvent::Disconnected { connection_epoch } => {
                    assert_eq!(connection_epoch, 7);
                    disconnected += 1;
                }
                other => panic!("unexpected authenticated lock event: {other:?}"),
            }
        }
        if !revoked
            && connected == 1
            && std::fs::read_to_string(&report).ok().as_deref() == Some("waiting\n")
        {
            assert_eq!(
                connected, 1,
                "independent authenticated peer must reach lock negotiation after Tversion"
            );
            f.book.revoke(id, Instant::now()).unwrap();
            revoked = true;
        }
        if supervisor.poll().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        supervisor.take_exit_status().unwrap().success(),
        "independent lock authentication peer must finish its wire assertions"
    );
    assert!(revoked);
    assert_eq!(
        std::fs::read_to_string(&report).unwrap(),
        "revoked\n",
        "a retained waiting read must lose authority on launch revocation"
    );
    let end = deadline();
    while disconnected == 0 {
        f.tick(end);
        if let Some(event) = service.try_event().unwrap() {
            match event {
                LockFileServiceEvent::Disconnected {
                    connection_epoch: 7,
                } => disconnected += 1,
                other => panic!("unexpected retirement event: {other:?}"),
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(disconnected, 1);
}
