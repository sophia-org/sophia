//! Kernel socket admission tests. Evidence is synthetic here; actual protected
//! supervisor launch is a separate production gate.
use sophia_9p::Errno;
use sophia_protocol::broker_files::*;
use sophia_runtime::{
    ProtectionBackendKind, ProtectionDomainEvidence, ProtectionDomainRole, broker_files::*,
};
use std::{
    io::Read,
    os::unix::net::UnixStream,
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
struct Owner;
impl BrokerFileEffects for Owner {
    type Prepared = ();
    fn prepare(&mut self, _: &BrokerFileResponseSet<'_>) -> Result<(), Errno> {
        Ok(())
    }
    fn commit(&mut self, _: ()) {}
}
fn evidence() -> ProtectionDomainEvidence {
    ProtectionDomainEvidence {
        backend: ProtectionBackendKind::Bubblewrap,
        supervisor_pid: std::process::id(),
        peer_pid: std::process::id(),
        roles: [ProtectionDomainRole::MetadataBroker].into_iter().collect(),
    }
}
fn listener(name: &str) -> BrokerFileListener {
    let directory = std::env::temp_dir().join(format!(
        "sophia-broker-file-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    BrokerFileListener::bind(directory, rustix::process::geteuid().as_raw()).unwrap()
}
#[test]
fn service_accepts_exact_kernel_peer_and_cleans_endpoint() {
    let listener = listener("exact");
    let path = listener.socket_path().to_owned();
    let client = UnixStream::connect(&path).unwrap();
    let mut service = listener
        .accept(
            Owner,
            &evidence(),
            7,
            1,
            0,
            BrokerFileLimits::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(!service.export().custody().closed());
    drop(client);
    for _ in 0..10 {
        if !service.turn(Duration::from_millis(10)).unwrap() {
            break;
        }
    }
    assert!(service.export().custody().closed());
    drop(service);
    assert!(!path.exists());
}

#[test]
fn owner_failure_ends_service_without_waiting_for_client_traffic() {
    struct FailedOwner(bool);
    impl BrokerFileEffects for FailedOwner {
        type Prepared = ();
        fn prepare(&mut self, _: &BrokerFileResponseSet<'_>) -> Result<(), Errno> {
            Ok(())
        }
        fn commit(&mut self, _: ()) {}
        fn failed(&self) -> bool {
            self.0
        }
    }
    let listener = listener("owner-failure");
    let mut client = UnixStream::connect(listener.socket_path()).unwrap();
    let mut service = listener
        .accept(
            FailedOwner(false),
            &evidence(),
            7,
            1,
            0,
            BrokerFileLimits::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    service.export_mut().owner_mut().0 = true;
    assert!(!service.turn(Duration::from_secs(30)).unwrap());
    assert!(service.export().custody().closed());
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    assert_eq!(client.read(&mut [0]).unwrap(), 0);
    drop(service);
}
#[test]
fn missing_protection_role_is_rejected() {
    let listener = listener("role");
    let mut evidence = evidence();
    evidence.roles.clear();
    assert!(
        listener
            .accept(
                Owner,
                &evidence,
                7,
                1,
                0,
                BrokerFileLimits::default(),
                Duration::from_millis(10)
            )
            .is_err()
    );
}
#[test]
fn foreign_process_is_rejected_before_attach() {
    let listener = listener("foreign");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "foreign_connection_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("SOPHIA_TEST_BROKER_SOCKET", listener.socket_path())
        .spawn()
        .unwrap();
    let result = listener.accept(
        Owner,
        &evidence(),
        7,
        1,
        0,
        BrokerFileLimits::default(),
        Duration::from_secs(2),
    );
    assert!(result.is_err());
    assert!(child.wait().unwrap().success());
}
#[test]
#[ignore = "subprocess of foreign_process_is_rejected_before_attach"]
fn foreign_connection_helper() {
    let path = std::env::var_os("SOPHIA_TEST_BROKER_SOCKET").expect("parent socket");
    let mut stream = UnixStream::connect(path).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut byte = [0; 1];
    assert_eq!(stream.read(&mut byte).unwrap(), 0);
}
