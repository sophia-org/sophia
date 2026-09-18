use std::fs;
use std::path::PathBuf;
use std::process::Command;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
                "sophia-cli-diagnostics-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )))
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sophia"));
        command
            .env("XDG_STATE_HOME", &self.0)
            .env_remove("SOPHIA_DIAGNOSTIC_DIR")
            .env_remove("SOPHIA_DIAGNOSTIC_SESSION");
        command
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn private_application_cli_escapes_by_default_and_requires_explicit_export() {
    use sophia_session::diagnostics::application::{
        ApplicationCapture, LaunchContext, LaunchSource,
    };
    use sophia_session::diagnostics::{Retention, Store};
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.0).unwrap();
    let store = Store::open(&fixture.0.join("sophia"), Retention::default()).unwrap();
    let run = store.begin("test", std::process::id(), "").unwrap();
    let capture = ApplicationCapture::start(&run.path).unwrap();
    capture.set_enabled(true);
    let mut command = Command::new("/bin/sh");
    command.args(["-c", r"printf '\377\033[31mprivate\n' >&2"]);
    let mut child = capture
        .spawn(
            &mut command,
            LaunchContext {
                source: LaunchSource::Startup,
                transaction: None,
            },
        )
        .unwrap();
    let ticket = capture.registration(child.id());
    let status = child.wait().unwrap();
    capture.exited(ticket, status);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !store
        .application_records(&run.id)
        .unwrap()
        .launches
        .get(&1)
        .is_some_and(|m| m.contains("eof=true"))
    {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    drop(capture);
    for (args, raw) in [
        (vec!["session", "stderr", "latest", "--launch=1"], false),
        (
            vec!["session", "stderr", "latest", "--launch=1", "--raw"],
            true,
        ),
    ] {
        let result = fixture.command().args(args).output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        if raw {
            assert_eq!(result.stdout, b"\xff\x1b[31mprivate\n");
        } else {
            assert!(!result.stdout.contains(&0x1b));
            assert!(
                String::from_utf8(result.stdout)
                    .unwrap()
                    .contains(r"\xff\x1b[31mprivate\n")
            );
        }
    }
    let result = fixture
        .command()
        .args(["session", "launches", "latest"])
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("launch=1 source=startup")
    );
    let result = fixture
        .command()
        .args(["session", "keep", "latest", "--include-application-stderr"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let path = String::from_utf8(result.stdout).unwrap();
    assert!(
        std::path::Path::new(path.trim().strip_prefix("preserved=").unwrap())
            .join("application-stderr.0.bin")
            .exists()
    );
}

#[test]
fn failed_wrapper_exit_is_retained_and_marked_without_a_running_desktop() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .args([
            "session",
            "_supervise",
            "--profile=test",
            "--",
            "/bin/sh",
            "-c",
            "exit 23",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(23),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = fixture
        .command()
        .args([
            "session",
            "mark",
            "--session=latest",
            "previous session crashed",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("marker="));
    let output = fixture
        .command()
        .args(["session", "inspect", "latest"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("status=failed"));
    assert!(text.contains("exit_status=23"));
    assert!(text.contains("previous session crashed"));
    assert!(!text.contains("owner_pid="));
    assert!(
        fixture
            .command()
            .args(["session", "keep", "latest"])
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn diagnostics_failure_does_not_replace_the_child_exit_status() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.0).unwrap();
    fs::write(fixture.0.join("sophia"), "not a directory").unwrap();
    let output = fixture
        .command()
        .args([
            "session",
            "_supervise",
            "--profile=test",
            "--",
            "/bin/sh",
            "-c",
            "exit 19",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(19),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("status=unavailable"));
}

#[test]
fn malformed_incident_commands_fail_without_starting_a_session() {
    let fixture = Fixture::new();
    for args in [
        vec!["session", "mark"],
        vec!["session", "mark", "--session=latest", "--session=other"],
        vec!["session", "inspect"],
        vec!["session", "keep"],
    ] {
        let output = fixture.command().args(args).output().unwrap();
        assert!(!output.status.success());
    }
}
