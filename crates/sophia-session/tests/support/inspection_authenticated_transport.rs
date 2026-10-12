//! Real launch channel, Factotum and inspection owner. The fixture assigns an
//! inspection audience to a protected generic peer; it does not qualify recipe
//! selection or grant inspection implicitly to production shell launches.
use super::lock_authenticated_transport::{Directory, Peer};
use super::*;
use sophia_runtime::ProtectionPath;
use sophia_runtime::inspection::*;
use sophia_session::launch_authentication::files::TransportBudget;
use std::os::unix::{fs::DirBuilderExt, net::UnixListener};

#[path = "inspection_rust_peer.rs"]
mod rust_peer;

#[test]
fn protected_peer_reads_inspection_and_revocation_ends_its_waiting_read() {
    let peer = Peer::rust_child(
        "inspection_authenticated_transport::rust_peer::protected_inspection_child",
    );
    let root =
        Directory(std::env::temp_dir().join(format!("t318-inspection-{}", std::process::id())));
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
    let endpoint = root.0.join("endpoint");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&endpoint)
        .unwrap();
    let socket = endpoint.join("inspection");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut service = InspectionService::private().unwrap();
    service.fence(9, None).unwrap();
    service
        .publisher()
        .publish(
            InspectionSnapshot {
                session_generation: 31,
                wm_epoch: 9,
                scene_generation: 4,
                selected_capabilities: 0,
                wire: InspectionWire::Files,
                state: InspectionState::Ready,
                outputs: vec![],
                surfaces: vec![],
            },
            None,
        )
        .unwrap();
    let mut f = Fixture::new();
    let id = f.reserve();
    let executable = std::env::current_exe().unwrap();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
        .unwrap()
        .path(ProtectionPath::read_only(&endpoint))
        .unwrap()
        .path(ProtectionPath::read_write(&reports))
        .unwrap()
        .path(ProtectionPath::read_only(&executable))
        .unwrap();
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::Shell,
        ProcessLaunchSpec::new(&peer.0)
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
        .bind(worker, b"observer".to_vec(), b"inspection".to_vec())
        .unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let stream = loop {
        f.tick(end);
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(1))
            }
            Err(e) => panic!("inspection accept: {e}"),
        }
    };
    // Availability filtering remains distinct from the afid authority proof.
    let credentials = rustix::net::sockopt::socket_peercred(&stream).unwrap();
    assert_eq!(credentials.uid, rustix::process::geteuid());
    assert_eq!(
        credentials.pid.as_raw_nonzero().get() as u32,
        supervisor.protection_evidence().unwrap().peer_pid
    );
    let mut connection = service
        .adopt_authenticated(stream, service.generation().unwrap(), Box::new(factory))
        .unwrap();
    let mut revoked = false;
    let mut closed = false;
    loop {
        f.tick(end);
        closed |= !connection.poll().unwrap();
        if !revoked && std::fs::read_to_string(&report).ok().as_deref() == Some("waiting\n") {
            assert!(connection.admitted());
            assert_eq!(f.namespaces.lock().unwrap().admission_count(), 1);
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
        "protected inspection exchange must finish"
    );
    assert!(
        revoked && closed,
        "launch revocation must close the observer transport"
    );
    assert_eq!(std::fs::read_to_string(report).unwrap(), "revoked\n");
    while f.namespaces.lock().unwrap().admission_count() != 0 {
        f.tick(end);
    }
}
