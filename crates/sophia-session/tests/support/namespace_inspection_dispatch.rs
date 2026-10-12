//! Independent Rust SDK child through the production recipe launch/dispatch.
#![cfg(test)]
use super::*;
use sophia_9p_client::pipeline::{Pipeline, Reply};
use sophia_9p_client::records::Fid;
use sophia_runtime::inspection::*;

struct Bootstrap(PathBuf);
impl Drop for Bootstrap {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn a_retired_inspection_service_does_not_revoke_the_clients_other_grant() {
    let mut f = Fixture::new();
    std::fs::write(
        f.root.join("recipe"),
        "namespace-recipe 1\nmount inspection roles/status\nmount portal roles/portal\n",
    )
    .unwrap();
    let mut service = InspectionService::private().unwrap();
    service.fence(9, None).unwrap();
    let mut request = f.request();
    request
        .approved
        .get_mut("inspection")
        .unwrap()
        .service
        .generation = service.generation().unwrap();
    let reference = request.approved["inspection"].service.clone();
    let mut portal = request.approved["inspection"].clone();
    portal.service.identity += 1;
    portal.service.interface = "portal-requester".into();
    portal.audience += 1;
    request.approved.insert("portal".into(), portal);
    let mut pending = f
        .service
        .start(
            request,
            &mut f.auth.coordinator,
            f.wake.notifier(),
            Instant::now(),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut running = loop {
        assert!(Instant::now() < deadline, "client startup deadline");
        f.auth.tick();
        if let Some(running) = pending
            .poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now())
            .unwrap()
        {
            break running;
        }
    };
    running
        .serve_inspection(
            &reference,
            service
                .authenticated_acceptor(reference.generation)
                .unwrap(),
            f.wake.notifier(),
        )
        .unwrap();
    let endpoints = running.take_endpoints();
    assert_eq!(endpoints.len(), 1);
    assert_eq!(
        endpoints[0].listener.endpoint.endpoint.service.interface,
        "portal-requester"
    );
    service.fence(10, None).unwrap();
    while !running.services.is_empty() {
        assert!(
            Instant::now() < deadline,
            "retired inspection dispatcher exits"
        );
        f.auth.tick();
        assert!(
            running.poll(&f.auth.coordinator).unwrap().is_none(),
            "service retirement must not terminate the client"
        );
    }
    assert!(
        endpoints[0].grant.is_live(),
        "unrelated portal grant survives inspection retirement"
    );
    assert_eq!(
        f.auth
            .coordinator
            .phase(running.registration.as_ref().unwrap().registration()),
        Some(Phase::Active)
    );
    drop(running);
    f.reclaimed();
}

#[test]
fn recipe_dispatch_delivers_inspection_and_revokes_a_waiting_sdk_read() {
    let mut f = Fixture::new();
    let executable = std::env::current_exe().unwrap();
    let bootstrap = Bootstrap(
        executable.with_file_name(format!("recipe-observer-bootstrap-{}", std::process::id())),
    );
    let status = std::process::Command::new("/usr/bin/cc")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-O2"])
        .arg("-DSOPHIA_TEST_CHILD=\"namespace_launch::tests::dispatch::recipe_observer_child\"")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/lock_rust_bootstrap.c"
        ))
        .arg("-o")
        .arg(&bootstrap.0)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "descriptor bootstrap compiles in the leased target"
    );
    let reports = f.root.join("reports");
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
    let mut request = f.request();
    request
        .approved
        .get_mut("inspection")
        .unwrap()
        .service
        .generation = service.generation().unwrap();
    let reference = request.approved["inspection"].service.clone();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::NamespaceClient])
        .unwrap()
        .path(ProtectionPath::read_only(&executable))
        .unwrap()
        .path(ProtectionPath::read_write(&reports))
        .unwrap();
    request.launch = ProcessLaunchSpec::new(&bootstrap.0)
        .arg(&executable)
        .arg("unused")
        .arg(&report)
        .protection_domain(domain);
    let mut pending = f
        .service
        .start(
            request,
            &mut f.auth.coordinator,
            f.wake.notifier(),
            Instant::now(),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut running = loop {
        assert!(Instant::now() < deadline, "SDK client startup deadline");
        f.auth.tick();
        if let Some(running) = pending
            .poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now())
            .unwrap()
        {
            break running;
        }
    };
    running
        .serve_inspection(
            &reference,
            service
                .authenticated_acceptor(reference.generation)
                .unwrap(),
            f.wake.notifier(),
        )
        .unwrap();
    assert!(running.take_endpoints().is_empty());
    while std::fs::read_to_string(&report).ok().as_deref() != Some("waiting\n") {
        assert!(
            Instant::now() < deadline,
            "SDK child must read snapshot before waiting"
        );
        f.auth.tick();
        assert!(running.poll(&f.auth.coordinator).unwrap().is_none());
    }
    running.names.as_mut().unwrap().revoke_grants();
    while std::fs::read_to_string(&report).ok().as_deref() != Some("revoked\n") {
        assert!(
            Instant::now() < deadline,
            "revocation must wake the retained SDK read"
        );
        f.auth.tick();
    }
    loop {
        assert!(
            Instant::now() < deadline,
            "SDK child must exit successfully"
        );
        f.auth.tick();
        let supervisor = running.supervisor.as_mut().unwrap();
        supervisor.poll().unwrap();
        if let Some(status) = supervisor.take_exit_status() {
            assert!(status.success());
            break;
        }
    }
    drop(running);
    f.reclaimed();
}

fn open(pipe: &mut Pipeline, root: Fid, name: &[u8]) -> Fid {
    let (tag, fid) = pipe.walk(root, &[name]).unwrap();
    assert!(
        matches!(pipe.wait(tag, Instant::now() + Duration::from_secs(3)).unwrap(), Reply::Walk(qids) if qids.len() == 1)
    );
    let tag = pipe.lopen(fid, 0).unwrap();
    assert!(matches!(
        pipe.wait(tag, Instant::now() + Duration::from_secs(3))
            .unwrap(),
        Reply::Lopen { .. }
    ));
    fid
}

#[test]
#[ignore = "parent launches this independent SDK child through the recipe scheduler"]
fn recipe_observer_child() {
    let directory = std::env::var_os(NAMESPACE_DIRECTORY_ENV).unwrap();
    let report = std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap();
    let stdin = std::io::stdin();
    rustix::io::fcntl_setfd(&stdin, rustix::io::FdFlags::CLOEXEC).unwrap();
    let relay =
        std::os::unix::net::UnixStream::from(rustix::io::fcntl_dupfd_cloexec(&stdin, 3).unwrap());
    let names = sophia_namespace_client::Namespace::open(std::path::Path::new(&directory)).unwrap();
    let mut client = sophia_launch_client::Client::over(
        names.connect("roles/status").unwrap(),
        relay,
        NAMESPACE_CLIENT_UNAME,
        b"inspection",
    )
    .unwrap();
    while !client.ready() {
        client.poll().unwrap();
        client.wait_for_io(Duration::from_millis(20)).unwrap();
    }
    let mut admitted = client.into_authenticated().unwrap();
    let snapshot = open(&mut admitted.role, admitted.root, b"snapshot");
    let tag = admitted.role.read(snapshot, 0, 65_000).unwrap();
    let Reply::Read(bytes) = admitted
        .role
        .wait(tag, Instant::now() + Duration::from_secs(3))
        .unwrap()
    else {
        panic!("inspection snapshot");
    };
    let snapshot = sophia_protocol::inspection::decode_inspection_snapshot(&bytes).unwrap();
    assert_eq!(snapshot.snapshot.scene_generation, 4);
    let events = open(&mut admitted.role, admitted.root, b"events");
    let pending = admitted
        .role
        .read(events, snapshot.event_offset, 4096)
        .unwrap();
    admitted.role.poll().unwrap();
    std::fs::write(&report, "waiting\n").unwrap();
    match admitted
        .role
        .wait(pending, Instant::now() + Duration::from_secs(5))
    {
        Ok(Reply::Error(errno)) => assert_eq!(errno.0, 116),
        Err(sophia_9p_client::pipeline::PipelineError::Io(
            std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::BrokenPipe,
        )) => {}
        other => panic!("revoked inspection read returned {other:?}"),
    }
    std::fs::write(report, "revoked\n").unwrap();
}
