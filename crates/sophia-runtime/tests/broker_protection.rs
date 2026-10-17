//! Opt-in real supervisor admission for the native broker service. This checks
//! launch isolation and file access, not the broker reducer or desktop behavior.
use sophia_9p::Errno;
use sophia_protocol::broker_files::{BrokerFileLimits, BrokerFileResponseSet};
use sophia_runtime::{
    ProcessLaunchSpec, ProcessSupervisor, ProtectionDomainRole, ProtectionDomainSpec,
    ProtectionPath, SupervisedProcessKind, SupervisorCommand, broker_files::*,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Owner;
impl BrokerFileEffects for Owner {
    type Prepared = ();
    fn prepare(&mut self, _: &BrokerFileResponseSet<'_>) -> Result<(), Errno> {
        Ok(())
    }
    fn commit(&mut self, _: ()) {}
}
struct Marker(PathBuf);
impl Drop for Marker {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
#[ignore = "requires SOPHIA_TEST_BUBBLEWRAP naming a qualified absolute executable"]
fn protected_broker_is_admitted_from_real_supervisor_evidence() {
    let bubblewrap =
        PathBuf::from(std::env::var_os("SOPHIA_TEST_BUBBLEWRAP").expect("bubblewrap path"));
    assert!(bubblewrap.is_absolute());
    let unique = format!(
        "sophia-native-broker-protected-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let marker = Marker(std::env::temp_dir().join(format!("{unique}-hidden")));
    std::fs::write(&marker.0, b"host only").unwrap();
    let listener = BrokerFileListener::bind(
        std::env::temp_dir().join(unique),
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataBroker])
        .unwrap()
        .bubblewrap_path(bubblewrap)
        .path(ProtectionPath::read_only(
            listener.socket_path().parent().unwrap(),
        ))
        .unwrap();
    let spec = ProcessLaunchSpec::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("protected_broker_peer")
        .arg("--ignored")
        .arg("--nocapture")
        .env(SOPHIA_BROKER_9P_SOCKET_ENV, listener.socket_path())
        .env("SOPHIA_TEST_HIDDEN_MARKER", &marker.0)
        .env(
            "SOPHIA_TEST_HOST_NETNS",
            std::fs::read_link("/proc/self/ns/net").unwrap(),
        )
        .process_group()
        .protection_domain(domain);
    let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::MetadataBroker, spec);
    supervisor
        .apply(SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::MetadataBroker,
            delay: Duration::ZERO,
        })
        .unwrap();
    let evidence = supervisor
        .protection_evidence()
        .expect("actual protected launch")
        .clone();
    assert_ne!(Some(evidence.peer_pid), supervisor.child_id());
    assert_eq!(Some(evidence.peer_pid), supervisor.peer_id());
    let mut service = listener
        .accept(
            Owner,
            &evidence,
            7,
            1,
            0,
            BrokerFileLimits::default(),
            Duration::from_secs(3),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while supervisor.poll().unwrap().is_none() {
        assert!(Instant::now() < deadline, "protected peer did not finish");
        service.turn(Duration::from_millis(10)).unwrap();
    }
    assert!(supervisor.exit_status().unwrap().success());
    service.close().unwrap();
    assert!(service.export().custody().closed());
}

#[test]
#[ignore = "child of protected_broker_is_admitted_from_real_supervisor_evidence"]
fn protected_broker_peer() {
    use sophia_9p_client::client::{Client, ClientLimits};
    for variable in ["DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS"] {
        assert!(std::env::var_os(variable).is_none(), "inherited {variable}");
    }
    for path in [
        PathBuf::from("/dev/dri"),
        PathBuf::from("/dev/input"),
        PathBuf::from(std::env::var_os("SOPHIA_TEST_HIDDEN_MARKER").unwrap()),
    ] {
        assert!(!path.exists(), "exposed {}", path.display());
    }
    assert_ne!(
        std::fs::read_link("/proc/self/ns/net").unwrap(),
        PathBuf::from(std::env::var_os("SOPHIA_TEST_HOST_NETNS").unwrap())
    );
    let path = PathBuf::from(std::env::var_os(SOPHIA_BROKER_9P_SOCKET_ENV).unwrap());
    let mut client = Client::connect(&path, ClientLimits::default()).unwrap();
    let root = client.attach(b"", b"").unwrap();
    let mut api = client.walk(&root, &[b"api"]).unwrap();
    client.open(&mut api, false).unwrap();
    assert_eq!(
        client.read_to_end(&api, 128).unwrap(),
        b"sophia-broker-files version=1 role=metadata-broker epoch=7 fd_transfer=none\n"
    );
    client.clunk(api).unwrap();
    client.clunk(root).unwrap();
}
