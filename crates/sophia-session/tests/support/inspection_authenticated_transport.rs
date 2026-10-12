//! Real launch channel, Factotum and inspection owner. The fixture assigns an
//! inspection audience to a protected namespace client; it does not qualify
//! automatic recipe selection or grant inspection implicitly to other roles.
use super::lock_authenticated_transport::{Directory, Peer};
use super::*;
use sophia_protocol::namespace_view::{EndpointProtocol, ServiceAvailability, ServiceReference};
use sophia_runtime::ProtectionPath;
use sophia_runtime::inspection::*;
use sophia_session::launch_authentication::files::TransportBudget;
use sophia_session::namespace_views::{ApprovedEndpoint, NamespaceRecipeOwner};
use std::os::unix::fs::DirBuilderExt;

#[path = "inspection_rust_peer.rs"]
mod rust_peer;

#[path = "inspection_namespace_rebind.rs"]
mod rebind;

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
    let id = f.reserve_role_audiences(ProtectionDomainRole::NamespaceClient, &[4, 5]);
    let mut names = NamespaceRecipeOwner::new(
        &root.0,
        f.namespaces.clone(),
        sophia_protocol::NamespaceId::from_raw(9),
        8,
        id.generation(),
    )
    .unwrap();
    std::fs::write(
        root.0.join("recipe"),
        "namespace-recipe 1\nmount first inspection\nmount second second\n",
    )
    .unwrap();
    let recipe =
        sophia_config::namespace_recipe::PinnedNamespaceRecipe::load(&root.0, "recipe").unwrap();
    let approved = [("first", 4), ("second", 5)]
        .into_iter()
        .map(|(name, audience)| {
            (
                name.into(),
                ApprovedEndpoint {
                    service: ServiceReference {
                        identity: 20,
                        generation: service.generation().unwrap(),
                        protocol: EndpointProtocol::NineP2000L,
                        interface: "inspection".into(),
                        version: 1,
                    },
                    audience,
                    availability: ServiceAvailability::Available,
                },
            )
        })
        .collect();
    let published = names.publish(1, &recipe, &approved).unwrap();
    let endpoint = published.namespace.lease.directory();
    let socket = endpoint.join("inspection");
    let listener = &published.namespace.listeners[0].listener;
    let second_listener = &published.namespace.listeners[1].listener;
    let executable = std::env::current_exe().unwrap();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::NamespaceClient])
        .unwrap()
        .path(ProtectionPath::read_only(endpoint))
        .unwrap()
        .path(ProtectionPath::read_write(&reports))
        .unwrap()
        .path(ProtectionPath::read_only(&executable))
        .unwrap();
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::NamespaceClient,
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
    let grants: Vec<_> = published
        .namespace
        .lease
        .view()
        .endpoints
        .iter()
        .map(|e| published.grant(&e.endpoint).unwrap().clone())
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
