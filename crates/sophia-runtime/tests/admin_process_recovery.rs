//! Abrupt process loss preserves uncertainty; recovery uses a fresh endpoint.
#[allow(dead_code)]
#[path = "support/shell_files_oracle/process.rs"]
mod process;
#[path = "support/admin_files.rs"]
mod support;
use sophia_runtime::AdminClient;
use std::process::{Command, Stdio};
use support::*;

#[test]
#[ignore = "child process for the foreign namespace admission proof"]
fn foreign_namespace_child() {
    let path = std::path::PathBuf::from(std::env::var_os("SOPHIA_ADMIN_DENIED_SOCKET").unwrap());
    // Prove path reachability separately from authenticated protocol admission.
    drop(UnixStream::connect(&path).unwrap());
    match AdminClient::connect(&path) {
        Ok(_) => panic!("foreign namespace admitted"),
        Err(error) => assert_ne!(error.kind(), std::io::ErrorKind::TimedOut),
    }
}

#[test]
#[ignore = "explicit nested namespace admission proof; requires bubblewrap"]
fn foreign_namespace_cannot_use_administrative_files() {
    let f = Fixture::new();
    assert_eq!(
        AdminClient::connect(f.service.socket_path())
            .unwrap()
            .commands()
            .unwrap(),
        catalog(7)
    );
    let status = Command::new("bwrap")
        .args([
            "--die-with-parent",
            "--unshare-user",
            "--unshare-pid",
            "--unshare-net",
            "--ro-bind",
            "/",
            "/",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
        ])
        .arg(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "foreign_namespace_child",
            "--nocapture",
        ])
        .env("SOPHIA_ADMIN_DENIED_SOCKET", f.service.socket_path())
        .status()
        .unwrap();
    assert!(
        status.success(),
        "namespace proof prerequisites or denial failed"
    );
    assert!(f.service.try_request().is_none());
}

#[test]
#[ignore = "child process owned by abrupt_process_loss_never_replays"]
fn crash_service_child() {
    let root = std::path::PathBuf::from(std::env::var_os("SOPHIA_ADMIN_CRASH_ROOT").unwrap());
    let service = ControlService::bind_files(&root).unwrap();
    until(|| service.publish(Arc::new(catalog(7)), &[]));
    std::fs::write(
        root.join("endpoint.tmp"),
        service.socket_path().as_os_str().as_encoded_bytes(),
    )
    .unwrap();
    std::fs::rename(root.join("endpoint.tmp"), root.join("endpoint")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let ticket = loop {
        if let Some(ticket) = service.try_request() {
            break ticket;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(ticket.command, action());
    if std::env::var("SOPHIA_ADMIN_CRASH_DISPATCH").as_deref() == Ok("1") {
        claim(&ticket);
        // This durable fixture marker represents an effect before a reply is visible.
        std::fs::write(root.join("effect"), b"one effect\n").unwrap();
    }
    std::fs::write(root.join("accepted"), b"ready\n").unwrap();
    loop {
        std::thread::park_timeout(Duration::from_secs(1));
    }
}

#[test]
fn abrupt_process_loss_never_replays() {
    use std::os::unix::fs::DirBuilderExt;
    for dispatched in [false, true] {
        let root =
            std::env::temp_dir().join(format!("admin-crash-{}-{dispatched}", std::process::id()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        let scratch = process::Scratch(root);
        let mut child = process::ChildGuard(
            Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "crash_service_child", "--nocapture"])
                .env("SOPHIA_ADMIN_CRASH_ROOT", &scratch.0)
                .env(
                    "SOPHIA_ADMIN_CRASH_DISPATCH",
                    if dispatched { "1" } else { "0" },
                )
                .stdin(Stdio::null())
                .spawn()
                .unwrap(),
        );
        until(|| scratch.0.join("endpoint").exists());
        let old_path =
            std::path::PathBuf::from(std::fs::read_to_string(scratch.0.join("endpoint")).unwrap());
        let mut client = AdminClient::connect(&old_path).unwrap();
        let old_info = client.info();
        let pending = std::thread::spawn(move || {
            assert!(client.invoke(action()).is_err());
            assert!(client.invocation_pending());
            client
        });
        until(|| scratch.0.join("accepted").exists());
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        let mut client = pending.join().unwrap();
        assert_eq!(scratch.0.join("effect").exists(), dispatched);
        assert!(AdminClient::connect(&old_path).is_err());
        let successor = ControlService::bind_files(&scratch.0).unwrap();
        until(|| successor.publish(Arc::new(catalog(7)), &[]));
        assert_ne!(successor.socket_path(), old_path);
        assert!(client.commands().is_err());
        assert!(client.invoke(action()).is_err());
        assert!(successor.try_request().is_none());
        let path = successor.socket_path().to_owned();
        let fresh = std::thread::spawn(move || {
            let mut client = AdminClient::connect(&path).unwrap();
            assert_ne!(client.info().session_id, old_info.session_id);
            assert_eq!(
                client.invoke(action()).unwrap().1,
                ControlOutcome::Committed
            );
        });
        let mut ticket = None;
        until(|| {
            ticket = successor.try_request();
            ticket.is_some()
        });
        let ticket = ticket.unwrap();
        claim(&ticket);
        ticket.finish(ControlOutcome::Committed);
        fresh.join().unwrap();
        assert!(successor.try_request().is_none());
    }
}
