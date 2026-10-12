//! Real launch channel, Factotum and inspection owner. The fixture assigns an
//! inspection audience to a protected generic peer; it does not qualify recipe
//! selection or grant inspection implicitly to production shell launches.
use super::lock_authenticated_transport::{Directory, Peer};
use super::*;
use sophia_protocol::namespace_view::{EndpointProtocol, ServiceReference};
use sophia_runtime::inspection::*;
use sophia_runtime::{EndpointGrantPolicy, ProtectionPath};
use sophia_session::launch_authentication::files::TransportBudget;
use std::os::unix::{fs::DirBuilderExt, net::UnixListener};

#[path = "inspection_rust_peer.rs"]
mod rust_peer;

#[test]
fn protected_peer_reads_inspection_and_revocation_ends_its_waiting_read() {
    run(false);
}

#[test]
fn one_endpoint_grant_revokes_without_stopping_the_other_authenticated_endpoint() {
    run(true);
}

fn run(grant_only: bool) {
    let peer = Peer::rust_child(if grant_only {
        "inspection_authenticated_transport::rust_peer::protected_two_endpoints_child"
    } else {
        "inspection_authenticated_transport::rust_peer::protected_inspection_child"
    });
    let root = Directory(std::env::temp_dir().join(format!(
        "t318-inspection-{grant_only}-{}",
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
    let report = reports.join("progress");
    let endpoint = root.0.join("endpoint");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&endpoint)
        .unwrap();
    let socket = endpoint.join("inspection");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let second_listener = UnixListener::bind(endpoint.join("second")).unwrap();
    second_listener.set_nonblocking(true).unwrap();
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
    let id = f.reserve_audiences(&[4, 5]);
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
    let grants: Vec<_> = [4, 5]
        .into_iter()
        .map(|audience| {
            f.namespaces
                .lock()
                .unwrap()
                .grant_endpoint(EndpointGrantPolicy {
                    namespace: sophia_protocol::NamespaceId::from_raw(9),
                    principal: 8,
                    launch_generation: id.generation(),
                    audience,
                    service: ServiceReference {
                        identity: 20,
                        generation: service.generation().unwrap(),
                        protocol: EndpointProtocol::NineP2000L,
                        interface: "inspection".into(),
                        version: 1,
                    },
                })
                .unwrap()
        })
        .collect();
    assert_eq!(grants[0].endpoint().service, grants[1].endpoint().service);
    assert_ne!(grants[0].endpoint().scope, grants[1].endpoint().scope);
    let budget = TransportBudget::new(f.namespaces.clone());
    let factory = budget
        .endpoint()
        .bind_grant(
            worker,
            grants[0].clone(),
            grants[0].endpoint(),
            b"observer".to_vec(),
            b"inspection".to_vec(),
        )
        .unwrap();
    let second_worker = f
        .book
        .worker_authority_for_audience(id, 5, &mut supervisor, Instant::now())
        .unwrap();
    let mut second_factory = Some(
        budget
            .endpoint()
            .bind_grant(
                second_worker,
                grants[1].clone(),
                grants[1].endpoint(),
                b"observer".to_vec(),
                b"inspection".to_vec(),
            )
            .unwrap(),
    );
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
    check_peer(&stream, &supervisor);
    let mut connection = service
        .adopt_authenticated(stream, service.generation().unwrap(), Box::new(factory))
        .unwrap();
    let mut revoked = false;
    let mut closed = false;
    let mut second: Option<InspectionConnection> = None;
    loop {
        f.tick(end);
        budget.poll_retiring(Instant::now());
        closed |= !connection.poll().unwrap();
        if second.is_none() && grant_only {
            match second_listener.accept() {
                Ok((stream, _)) => {
                    check_peer(&stream, &supervisor);
                    second = Some(
                        service
                            .adopt_authenticated(
                                stream,
                                service.generation().unwrap(),
                                Box::new(second_factory.take().unwrap()),
                            )
                            .unwrap(),
                    );
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(e) => panic!("second inspection accept: {e}"),
            }
        }
        if let Some(second) = &mut second {
            second.poll().unwrap();
        }
        if !revoked && std::fs::read_to_string(&report).ok().as_deref() == Some("waiting\n") {
            assert!(connection.admitted());
            assert_eq!(
                f.namespaces.lock().unwrap().admission_count(),
                if grant_only { 2 } else { 1 }
            );
            if grant_only {
                assert!(second.as_ref().unwrap().admitted());
                f.namespaces
                    .lock()
                    .unwrap()
                    .revoke_endpoint_grant(&grants[0])
                    .unwrap();
                assert!(grants[1].is_live());
                assert!(
                    second.as_ref().unwrap().admitted(),
                    "unrelated endpoint admission must remain live"
                );
            } else {
                f.book.revoke(id, Instant::now()).unwrap();
            }
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
        "revocation must close the selected observer transport"
    );
    assert_eq!(
        std::fs::read_to_string(report).unwrap(),
        if grant_only {
            "unrelated-progress\n"
        } else {
            "revoked\n"
        }
    );
    drop(second);
    drop(connection);
    while f.namespaces.lock().unwrap().admission_count() != 0 {
        f.tick(end);
        budget.poll_retiring(Instant::now());
    }
}

fn check_peer(stream: &UnixStream, supervisor: &ProcessSupervisor) {
    let credentials = rustix::net::sockopt::socket_peercred(stream).unwrap();
    assert_eq!(credentials.uid, rustix::process::geteuid());
    assert_eq!(
        credentials.pid.as_raw_pid() as u32,
        supervisor.protection_evidence().unwrap().peer_pid
    );
}
