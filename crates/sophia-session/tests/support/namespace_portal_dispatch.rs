//! SDK recipient through real launch authentication and the common portal lease.
//! Transfer policy and source bytes are supplied; no application frontend runs.
#![cfg(test)]
use super::*;
use sophia_9p_client::pipeline::{Pipeline, Reply};
use sophia_9p_client::records::Fid;
#[path = "../../../sophia-cli/src/commands/capture.rs"]
mod capture_cli;
#[path = "namespace_capture_fixture.rs"]
mod capture_fixture;
#[path = "../../../sophia-portal/tests/support/client_object_fixture.rs"]
mod objects;

struct Bootstrap(PathBuf);
impl Drop for Bootstrap {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn portal_boundaries_revoke_retained_sdk_reads_without_closing_the_role() {
    for boundary in ["generation", "lock", "replacement"] {
        run_boundary(boundary);
    }
}
#[test]
fn capture_cli_reads_one_image_through_real_namespace_launch_authentication() {
    run_boundary("capture");
}
#[test]
#[ignore = "requires SOPHIA_PORTAL_AUTH_PEER naming an independently built SDK peer"]
fn external_sdk_portal_startup_uses_real_namespace_authentication() {
    run_boundary("sdk-startup");
}
#[test]
#[ignore = "requires SOPHIA_TEST_CAPTURE_CLI naming the built generic Sophia CLI"]
fn capture_cli_bootstrap_uses_the_attested_launch_channel() {
    run_boundary("capture-bootstrap");
}
#[test]
#[ignore = "requires SOPHIA_TEST_CAPTURE_CLI naming the built generic Sophia CLI"]
fn capture_cli_reads_a_4k_image_within_the_existing_grant_deadline() {
    run_boundary("capture-bootstrap-large");
}
fn run_boundary(boundary: &str) {
    let external = boundary == "sdk-startup";
    let is_capture = boundary.starts_with("capture");
    let standalone = boundary.starts_with("capture-bootstrap");
    let (width, height) = if boundary.ends_with("-large") {
        (3840, 2160)
    } else {
        (2, 2)
    };
    let mut f = Fixture::with_capabilities(
        sophia_protocol::NamespaceProfile::Confined,
        sophia_protocol::NamespaceCapabilities::NONE.with_request(if is_capture {
            sophia_protocol::NamespacePortalCapability::ScreenCapture
        } else {
            sophia_protocol::NamespacePortalCapability::Clipboard
        }),
    );
    std::fs::write(
        f.root.join("recipe"),
        "namespace-recipe 1\nmount portal roles/portal\n",
    )
    .unwrap();
    let objects = objects::Fixture::new();
    let capture = is_capture.then(|| capture_fixture::Fixture::new(width, height));
    let executable = if external {
        let path = PathBuf::from(std::env::var_os("SOPHIA_PORTAL_AUTH_PEER").unwrap());
        assert!(path.is_absolute() && path.is_file());
        path
    } else {
        std::env::current_exe().unwrap()
    };
    let bootstrap = Bootstrap(executable.with_file_name(format!(
        "recipe-portal-bootstrap-{}-{boundary}",
        std::process::id()
    )));
    let mut compiler = std::process::Command::new("/usr/bin/cc");
    if external {
        compiler.arg("-DSOPHIA_TEST_EXTERNAL_PEER");
    }
    assert!(compiler.env_clear().env("PATH", "/usr/bin:/bin")
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-O2"])
        .arg("-DSOPHIA_TEST_CHILD=\"namespace_launch::tests::portal_dispatch::recipe_portal_child\"")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/lock_rust_bootstrap.c"))
        .arg("-o").arg(&bootstrap.0).status().unwrap().success());
    let reports = f.root.join("reports");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&reports)
        .unwrap();
    let report = reports.join("progress");
    let mut request = f.request();
    request.recipe = LoadedNamespaceRecipe::load(
        &NamespaceRecipeConfig {
            name: "recipient".into(),
            profile: NamespaceRecipeProfile::ConfinedX { group: 7 },
            source: Some("recipe".into()),
            request: vec![if is_capture {
                sophia_config::namespace_policy::RecipePortalCapability::ScreenCapture
            } else {
                sophia_config::namespace_policy::RecipePortalCapability::Clipboard
            }],
            publish: vec![],
            developer_launch: true,
            capture_outputs: vec![],
        },
        &f.root,
    )
    .unwrap()
    .into();
    let mut endpoint = request.approved.remove("inspection").unwrap();
    endpoint.service.interface = "portal-client".into();
    let reference = endpoint.service.clone();
    request.approved.insert("portal".into(), endpoint);
    request.launch = ProcessLaunchSpec::new(&bootstrap.0)
        .arg(&executable)
        .arg("unused")
        .arg(&report)
        .env("SOPHIA_TEST_PORTAL_BOUNDARY", boundary)
        .protection_domain(
            ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::NamespaceClient])
                .unwrap()
                .path(ProtectionPath::read_only(&executable))
                .unwrap()
                .path(ProtectionPath::read_write(&reports))
                .unwrap(),
        );
    if standalone {
        let cli =
            PathBuf::from(std::env::var_os("SOPHIA_TEST_CAPTURE_CLI").expect("built CLI path"));
        request.launch = ProcessLaunchSpec::new(&cli)
            .arg("capture")
            .arg("7")
            .arg("--output")
            .arg(report.with_extension("pam"))
            .protection_domain(
                ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::NamespaceClient])
                    .unwrap()
                    .path(ProtectionPath::read_write(&reports))
                    .unwrap(),
            );
    }
    let mut pending = f
        .service
        .start(
            request,
            &mut f.auth.coordinator,
            f.wake.notifier(),
            Instant::now(),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut running = loop {
        assert!(Instant::now() < deadline, "portal child startup deadline");
        f.auth.tick();
        if let Some(client) = pending
            .poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now())
            .unwrap()
        {
            break client;
        }
    };
    let foreign = Arc::new(sophia_portal::recipient_object::PortalObjectRecipient::new(
        NamespaceId::from_raw(99),
    ));
    assert!(
        running
            .serve_portal(&reference, foreign, f.wake.notifier())
            .is_err()
    );
    running
        .serve_portal(
            &reference,
            capture.as_ref().map_or_else(
                || objects.objects.clone(),
                |capture| capture.objects.clone(),
            ),
            f.wake.notifier(),
        )
        .unwrap();
    if external {
        while std::fs::read_to_string(&report).ok().as_deref() != Some("started\n") {
            assert!(
                Instant::now() < deadline,
                "external portal startup deadline"
            );
            f.auth.tick();
        }
    } else if let Some(capture) = capture {
        while if standalone {
            !report.with_extension("pam").exists()
        } else {
            std::fs::read_to_string(&report).ok().as_deref() != Some("captured\n")
        } {
            assert!(Instant::now() < deadline, "capture CLI deadline");
            f.auth.tick();
            capture.service();
        }
    } else {
        for phase in ["ready\n", "waiting\n", "revoked\n"] {
            while std::fs::read_to_string(&report).ok().as_deref() != Some(phase) {
                assert!(Instant::now() < deadline, "portal SDK phase {phase}");
                f.auth.tick();
            }
            if phase == "ready\n" {
                objects.transfer(1);
            }
            if phase == "waiting\n" {
                assert!(
                    objects.objects.current().is_ok(),
                    "grant is live before source change"
                );
                match boundary {
                    "generation" => objects.source.0.store(10, Ordering::Release),
                    "lock" => {
                        objects.fence.block();
                        objects.fence.unblock();
                    }
                    "replacement" => objects.transfer(2),
                    _ => unreachable!(),
                }
            }
        }
    }
    loop {
        assert!(Instant::now() < deadline, "portal child exit deadline");
        f.auth.tick();
        let supervisor = running.supervisor.as_mut().unwrap();
        supervisor.poll().unwrap();
        if let Some(status) = supervisor.take_exit_status() {
            assert!(status.success());
            break;
        }
    }
    if is_capture {
        let bytes = std::fs::read(report.with_extension("pam")).unwrap();
        assert_eq!(
            bytes,
            [
                format!("P7\nWIDTH {width}\nHEIGHT {height}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n").as_bytes(),
                &vec![17; width as usize * height as usize * 4]
            ]
            .concat()
        );
    }
    drop(running);
    f.reclaimed();
}
fn reply(pipe: &mut Pipeline, tag: sophia_9p_client::records::Tag) -> Reply {
    pipe.wait(tag, Instant::now() + Duration::from_secs(3))
        .unwrap()
}
fn open(pipe: &mut Pipeline, root: Fid, name: &[u8]) -> Fid {
    let (tag, fid) = pipe.walk(root, &[name]).unwrap();
    assert!(matches!(reply(pipe, tag), Reply::Walk(qids) if qids.len() == 1));
    let tag = pipe.lopen(fid, 0).unwrap();
    assert!(matches!(reply(pipe, tag), Reply::Lopen { .. }));
    fid
}
#[test]
#[ignore = "parent launches the SDK child through the namespace scheduler"]
fn recipe_portal_child() {
    let report = std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap();
    if std::env::var("SOPHIA_TEST_PORTAL_BOUNDARY").unwrap() == "capture" {
        let path = PathBuf::from(&report).with_extension("pam");
        capture_cli::run(&[
            "--relay-stdin".into(),
            "7".into(),
            "--output".into(),
            path.to_string_lossy().into_owned(),
        ])
        .unwrap();
        std::fs::write(report, "captured\n").unwrap();
        return;
    }
    let directory = std::env::var_os(NAMESPACE_DIRECTORY_ENV).unwrap();
    let stdin = std::io::stdin();
    rustix::io::fcntl_setfd(&stdin, rustix::io::FdFlags::CLOEXEC).unwrap();
    let relay =
        std::os::unix::net::UnixStream::from(rustix::io::fcntl_dupfd_cloexec(&stdin, 3).unwrap());
    let names = sophia_namespace_client::Namespace::open(std::path::Path::new(&directory)).unwrap();
    let mut client = sophia_launch_client::Client::over(
        names.connect("roles/portal").unwrap(),
        relay,
        NAMESPACE_CLIENT_UNAME,
        b"portal-client",
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !client.ready() {
        assert!(Instant::now() < deadline);
        client.poll().unwrap();
        client.wait_for_io(Duration::from_millis(20)).unwrap();
    }
    let mut admitted = client.into_authenticated().unwrap();
    let pipe = &mut admitted.role;
    let api = open(pipe, admitted.root, b"api");
    let tag = pipe.read(api, 0, 4096).unwrap();
    assert!(
        matches!(reply(pipe, tag), Reply::Read(bytes) if std::str::from_utf8(&bytes).unwrap().contains("request=unavailable"))
    );
    let (tag, _) = pipe.walk(admitted.root, &[b"upload"]).unwrap();
    assert!(matches!(reply(pipe, tag), Reply::Error(errno) if errno.0 == 2));
    std::fs::write(&report, "ready\n").unwrap();
    let transfer = loop {
        assert!(Instant::now() < deadline);
        let (tag, fid) = pipe.walk(admitted.root, &[b"transfer"]).unwrap();
        match reply(pipe, tag) {
            Reply::Walk(qids) if qids.len() == 1 => break fid,
            Reply::Error(errno) if errno.0 == 2 => std::thread::sleep(Duration::from_millis(10)),
            other => panic!("transfer publication: {other:?}"),
        }
    };
    let tag = pipe.lopen(transfer, 0).unwrap();
    assert!(matches!(reply(pipe, tag), Reply::Lopen { .. }));
    let tag = pipe.read(transfer, 0, 4096).unwrap();
    assert!(matches!(reply(pipe, tag), Reply::Read(bytes) if bytes == b"granted"));
    let watch = open(pipe, admitted.root, b"watch");
    let pending = pipe.read(watch, 0, 1).unwrap();
    pipe.poll().unwrap();
    std::fs::write(&report, "waiting\n").unwrap();
    assert!(
        matches!(pipe.wait(pending, Instant::now() + Duration::from_secs(1)), Ok(Reply::Error(errno)) if errno.0 == 116),
        "transfer retirement must wake the retained watch with ESTALE"
    );
    let tag = pipe.read(transfer, 0, 4096).unwrap();
    assert!(matches!(reply(pipe, tag), Reply::Error(errno) if errno.0 == 116));
    let status = open(pipe, admitted.root, b"status");
    let tag = pipe.read(status, 0, 4096).unwrap();
    assert!(
        matches!(reply(pipe, tag), Reply::Read(_)),
        "object revocation must preserve the authenticated role"
    );
    if std::env::var("SOPHIA_TEST_PORTAL_BOUNDARY").unwrap() == "replacement" {
        let fresh = open(pipe, admitted.root, b"transfer");
        let tag = pipe.read(fresh, 0, 4096).unwrap();
        assert!(
            matches!(reply(pipe, tag), Reply::Read(bytes) if bytes == b"granted"),
            "fresh fid reaches the replacement while the retained fid remains stale"
        );
    }
    std::fs::write(report, "revoked\n").unwrap();
}
