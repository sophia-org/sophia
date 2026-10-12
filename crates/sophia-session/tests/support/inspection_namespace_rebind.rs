//! Rebinding naming keeps old authenticated roots and open snapshots on A.
use super::super::lock_authenticated_transport::rust_peer::{authenticate, open, read};
use super::*;
use sophia_9p_client::pipeline::Pipeline;
use sophia_9p_client::records::Fid;
use sophia_launch_client::Client;
use sophia_session::namespace_views::PublishedRecipe;

fn publish(service: &InspectionService, scene: u64) {
    service
        .publisher()
        .publish(
            InspectionSnapshot {
                session_generation: 31,
                wm_epoch: 9,
                scene_generation: scene,
                selected_capabilities: 0,
                wire: InspectionWire::Files,
                state: InspectionState::Ready,
                outputs: vec![],
                surfaces: vec![],
            },
            None,
        )
        .unwrap();
}

#[test]
fn rebind_preserves_the_authenticated_root_and_open_fid() {
    let peer = Peer::rust_child("inspection_authenticated_transport::rebind::rebind_child");
    let root = Directory(std::env::temp_dir().join(format!("t318-rebind-{}", std::process::id())));
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
    let mut a = InspectionService::private().unwrap();
    let mut b = InspectionService::private().unwrap();
    a.fence(9, None).unwrap();
    b.fence(9, None).unwrap();
    publish(&a, 4);
    publish(&b, 9);
    let mut f = Fixture::new();
    let id = f.reserve_role_audiences(ProtectionDomainRole::NamespaceClient, &[4]);
    let mut names = NamespaceRecipeOwner::new(
        &root.0,
        f.namespaces.clone(),
        sophia_protocol::NamespaceId::from_raw(9),
        8,
        id.generation(),
    )
    .unwrap();
    let approved = [("a", 20, &a), ("b", 30, &b)]
        .into_iter()
        .map(|(name, identity, service)| {
            (
                name.into(),
                ApprovedEndpoint {
                    service: ServiceReference {
                        identity,
                        generation: service.generation().unwrap(),
                        protocol: EndpointProtocol::NineP2000L,
                        interface: "inspection".into(),
                        version: 1,
                    },
                    audience: 4,
                    availability: ServiceAvailability::Available,
                },
            )
        })
        .collect();
    std::fs::write(
        root.0.join("first"),
        "namespace-recipe 1\nmount a inspection\nbind inspection alias\n",
    )
    .unwrap();
    let recipe =
        sophia_config::namespace_recipe::PinnedNamespaceRecipe::load(&root.0, "first").unwrap();
    let old = names.publish(1, &recipe, &approved).unwrap();
    let socket = old.namespace.lease.directory().join("inspection");
    let executable = std::env::current_exe().unwrap();
    // Bind the owner directory, so both immutable generations are visible.
    // The peer never chooses policy or follows the publisher's current symlink.
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::NamespaceClient])
        .unwrap()
        .path(ProtectionPath::read_only(names.directory()))
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
    let budget = TransportBudget::new(f.namespaces.clone());
    let mut next: Option<PublishedRecipe> = None;
    let mut connections = Vec::new();
    let mut accepted = std::collections::BTreeSet::new();
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        f.tick(end);
        budget.poll_retiring(Instant::now());
        for connection in &mut connections {
            let connection: &mut InspectionConnection = connection;
            connection.poll().unwrap();
        }
        for (revision, published) in [(1, Some(&old)), (2, next.as_ref())] {
            let Some(published) = published else {
                continue;
            };
            for (index, listener) in published.namespace.listeners.iter().enumerate() {
                if accepted.contains(&(revision, index)) {
                    continue;
                }
                let stream = match listener.listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                    Err(e) => panic!("namespace listener: {e}"),
                };
                check_peer(&stream, &supervisor);
                let endpoint = &listener.endpoint.endpoint;
                let grant = published.grant(endpoint).unwrap().clone();
                let worker = f
                    .book
                    .worker_authority_for_audience(id, 4, &mut supervisor, Instant::now())
                    .unwrap();
                let factory = budget
                    .endpoint()
                    .bind_grant(
                        worker,
                        grant,
                        endpoint,
                        b"observer".to_vec(),
                        b"inspection".to_vec(),
                    )
                    .unwrap();
                let service = match endpoint.service.identity {
                    20 => &a,
                    30 => &b,
                    _ => panic!("unapproved service"),
                };
                connections.push(
                    service
                        .authenticated_acceptor(endpoint.service.generation)
                        .unwrap()
                        .adopt(stream, Box::new(factory))
                        .unwrap(),
                );
                accepted.insert((revision, index));
            }
        }
        if next.is_none() && std::fs::read_to_string(&report).ok().as_deref() == Some("pinned\n") {
            assert!(connections[0].admitted());
            // A changes its current snapshot too: a retained fid must retain
            // its bytes, while a fresh fid on the old root still reaches A.
            publish(&a, 5);
            std::fs::write(root.0.join("second"),
                "namespace-recipe 1\nmount a inspection\nbind inspection alias\nunmount inspection\nmount b inspection\nmount a retained\n").unwrap();
            let recipe =
                sophia_config::namespace_recipe::PinnedNamespaceRecipe::load(&root.0, "second")
                    .unwrap();
            let replacement = names.publish(2, &recipe, &approved).unwrap();
            let path = replacement.namespace.lease.directory().to_owned();
            next = Some(replacement);
            std::fs::write(reports.join("next"), path.as_os_str().as_encoded_bytes()).unwrap();
        }
        if supervisor.poll().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        supervisor.take_exit_status().unwrap().success(),
        "rebind peer must finish its correlated read checks"
    );
    assert_eq!(std::fs::read_to_string(report).unwrap(), "rebind-ok\n");
    assert_eq!(accepted.len(), 3);
    drop(connections);
    while f.namespaces.lock().unwrap().admission_count() != 0 {
        f.tick(end);
        budget.poll_retiring(Instant::now());
    }
}

fn scene(pipe: &mut Pipeline, fid: Fid) -> u64 {
    decode_inspection_snapshot(&read(pipe, fid, 0, 65_000))
        .unwrap()
        .snapshot
        .scene_generation
}

#[test]
#[ignore = "parent invokes the independent SDK peer with a protected launch relay"]
fn rebind_child() {
    let socket = std::env::var_os("SOPHIA_TEST_LOCK_SOCKET").unwrap();
    let report = PathBuf::from(std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap());
    let stdin = std::io::stdin();
    rustix::io::fcntl_setfd(&stdin, rustix::io::FdFlags::CLOEXEC).unwrap();
    let relay = UnixStream::from(rustix::io::fcntl_dupfd_cloexec(&stdin, 3).unwrap());
    let old =
        sophia_namespace_client::Namespace::open(std::path::Path::new(&socket).parent().unwrap())
            .unwrap();
    let mut first = authenticate(
        Client::over(
            old.connect("inspection").unwrap(),
            relay,
            b"observer",
            b"inspection",
        )
        .unwrap(),
    );
    let pinned = open(&mut first.role, first.root, b"snapshot", 0);
    assert_eq!(scene(&mut first.role, pinned), 4);
    std::fs::write(&report, "pinned\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let path = loop {
        if let Ok(path) = std::fs::read_to_string(report.with_file_name("next")) {
            break path;
        }
        assert!(
            Instant::now() < deadline,
            "replacement publication deadline"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    let new = sophia_namespace_client::Namespace::open(std::path::Path::new(&path)).unwrap();
    assert_eq!(
        scene(&mut first.role, pinned),
        4,
        "an open fid must keep A's pinned bytes after naming rebind"
    );
    // Inspection permits one pinned snapshot per connection. Release the old
    // fid before testing a new snapshot through the same admitted root.
    let tag = first.role.clunk(pinned).unwrap();
    assert!(matches!(
        first.role.wait(tag, deadline).unwrap(),
        sophia_9p_client::pipeline::Reply::Clunk
    ));
    let fresh = open(&mut first.role, first.root, b"snapshot", 0);
    assert_eq!(
        scene(&mut first.role, fresh),
        5,
        "a fresh fid on the retained root must still reach A"
    );
    let mut replacement = authenticate(
        Client::over_relay(
            new.connect("inspection").unwrap(),
            first.relay,
            b"observer",
            b"inspection",
        )
        .unwrap(),
    );
    let fresh = open(&mut replacement.role, replacement.root, b"snapshot", 0);
    assert_eq!(
        scene(&mut replacement.role, fresh),
        9,
        "a fresh connection through the rebound name must reach B"
    );
    let mut alias = authenticate(
        Client::over_relay(
            new.connect("alias").unwrap(),
            replacement.relay,
            b"observer",
            b"inspection",
        )
        .unwrap(),
    );
    let fresh = open(&mut alias.role, alias.root, b"snapshot", 0);
    assert_eq!(
        scene(&mut alias.role, fresh),
        5,
        "a bound alias must retain A instead of looking up the rebound name"
    );
    assert_eq!(new.discovery().endpoint("inspection").unwrap().service, 30);
    assert_eq!(new.discovery().endpoint("alias").unwrap().service, 20);
    assert_eq!(old.discovery().endpoint("inspection").unwrap().service, 20);
    std::fs::write(report, "rebind-ok\n").unwrap();
}
